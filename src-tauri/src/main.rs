#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod lumi;
mod music;
mod plugins;
#[cfg(target_os = "macos")]
mod touchbar;

use auth::DeviceCodeState;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::Digest;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::time::Instant;
use tauri::{Emitter, Manager, Window};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{sleep, Duration};

fn parse_version(s: &str) -> Vec<i32> {
    s.split('.')
        .filter_map(|p| p.parse::<i32>().ok())
        .collect()
}

fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let va = parse_version(a);
    let vb = parse_version(b);
    va.cmp(&vb)
}

fn version_ge(a: &str, b: &str) -> bool {
    version_cmp(a, b) != std::cmp::Ordering::Less
}

// ========== 辅助：带时间戳的下载日志（终端 + 版本目录 downloads/lumia.log） ==========
static DOWNLOAD_LOG_FILE: std::sync::OnceLock<std::sync::Mutex<Option<PathBuf>>> =
    std::sync::OnceLock::new();

fn set_download_log_path(path: PathBuf) {
    let mtx = DOWNLOAD_LOG_FILE.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut guard) = mtx.lock() {
        *guard = Some(path);
    }
}

fn dl_log(msg: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    eprintln!("[下载 {}s] {}", now, msg);
    if let Some(mtx) = DOWNLOAD_LOG_FILE.get() {
        if let Ok(guard) = mtx.lock() {
            if let Some(path) = guard.as_ref() {
                if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(f, "[{}] {}", now, msg);
                }
            }
        }
    }
}

// ========== 配置结构 ==========
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadConfig {
    pub version: String,
    pub display_name: String,
    pub loader: String,
    pub loader_version: String,
    pub install_fabric_api: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub username: String,
    pub java_path: Option<String>,
    pub max_memory: u32,
    pub use_rosetta: bool,
    pub game_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms_refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms_access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curseforge_api_key: Option<String>,
    /// 界面语言偏好：auto（跟随系统）/ zh-CN / en / ja。serde default 保证旧配置文件缺字段也能加载。
    #[serde(default = "default_language")]
    pub language: String,
    /// AI 助手开关
    #[serde(default)]
    pub ai_enabled: bool,
    /// AI API 地址（OpenAI 兼容，不含 /chat/completions 后缀）
    #[serde(default = "default_ai_base_url")]
    pub ai_base_url: String,
    /// AI API Key
    #[serde(default)]
    pub ai_api_key: String,
    /// AI 模型名
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
    /// AI 提供商标识（deepseek / openai / anthropic / ... / custom）
    #[serde(default = "default_ai_provider")]
    pub ai_provider: String,
    /// 上次看到的版本（用于展示更新日志）
    #[serde(default = "default_last_seen_version")]
    pub last_seen_version: String,
    /// 自动检查更新（启动时请求官方版本 API，发现新版本弹窗提示）
    #[serde(default = "default_true")]
    pub auto_update: bool,
    /// 用户自定义 Java 启动参数（设置→高级设置），空格分隔
    #[serde(default)]
    pub java_args: String,
    /// 是否曾用正版（Microsoft 账号）启动过游戏。
    /// 非大陆地区（或断网无法判断地区）时，离线模式是否放行的依据。
    #[serde(default)]
    pub ever_launched_online: bool,
    /// 最近一次成功判定是否位于中国大陆；断网无法查询时回落到它
    #[serde(default)]
    pub known_mainland: Option<bool>,
    /// 侧边栏项目可见性（key = home/version/download/terracotta/music/ai/plugin-center/settings）
    #[serde(default = "default_sidebar_visible")]
    pub sidebar_visible: HashMap<String, bool>,
    /// 主题颜色（十六进制颜色值 或 data:image 格式的自定义背景图）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_last_seen_version() -> String {
    String::new()
}

fn default_language() -> String {
    "auto".to_string()
}

fn default_ai_base_url() -> String {
    "https://api.deepseek.com".to_string()
}

fn default_sidebar_visible() -> HashMap<String, bool> {
    vec![
        "home", "version", "download", "terracotta", "music", "ai", "pluginCenter", "settings",
    ]
    .into_iter()
    .map(|k| (k.to_string(), true))
    .collect()
}

fn default_ai_model() -> String {
    "deepseek-chat".to_string()
}

fn default_ai_provider() -> String {
    "deepseek".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            username: "Player".to_string(),
            java_path: None,
            max_memory: 2048,
            use_rosetta: false,
            game_dir: None,
            ms_refresh_token: None,
            ms_access_token: None,
            ms_uuid: None,
            ms_username: None,
            curseforge_api_key: None,
            language: "auto".to_string(),
            ai_enabled: false,
            ai_base_url: "https://api.deepseek.com".to_string(),
            ai_api_key: String::new(),
            ai_model: "deepseek-chat".to_string(),
            ai_provider: "deepseek".to_string(),
            last_seen_version: String::new(),
            auto_update: true,
            java_args: String::new(),
            ever_launched_online: false,
            known_mainland: None,
            sidebar_visible: default_sidebar_visible(),
            theme_color: None,
        }
    }
}

// ========== 应用状态 ==========
pub struct AppState {
    pub config: Arc<Mutex<AppConfig>>,
    pub config_path: PathBuf,
    pub device_code: Arc<Mutex<Option<DeviceCodeState>>>,
    pub cancel_download: Arc<AtomicBool>,
}

/// 游戏进程是否在运行（launch_game spawn 成功置 true，退出置 false）。
/// 用于陶瓦联机开房前的提示（参考 HMCL：检测到游戏未运行会提示先启动游戏）。
static GAME_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ========== 版本清单项 ==========
#[derive(Serialize)]
struct VersionManifestItem {
    id: String,
    #[serde(rename = "releaseTime")]
    release_time: String,
    category: String,
}

// ========== 获取游戏目录 ==========
// Lumia 专属游戏目录（各平台独立，游戏数据放在 Lumia 文件夹内的 .minecraft 子目录）：
//   macOS:   ~/Library/Application Support/Lumia/.minecraft
//   Windows: %APPDATA%\Lumia\.minecraft
//   Linux:   ~/.local/share/Lumia/.minecraft
fn default_game_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/Lumia/.minecraft");
        }
    } else if cfg!(target_os = "linux") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/Lumia/.minecraft");
        }
    } else if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("Lumia/.minecraft");
    }
    // 兜底
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".lumia/.minecraft");
    }
    PathBuf::from("Lumia/.minecraft")
}

async fn get_game_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    let state = app_handle.state::<AppState>();
    // 用 try_lock 非阻塞获取配置锁：锁被占用时直接用默认目录，绝不让下载卡死在这里
    let custom_dir = match state.config.try_lock() {
        Ok(config) => config.game_dir.clone(),
        Err(_) => {
            eprintln!("警告: 配置锁被占用，使用默认游戏目录");
            None
        }
    };

    if let Some(ref custom_dir) = custom_dir {
        return PathBuf::from(custom_dir);
    }

    default_game_dir()
}

// ========== 辅助：SHA1 十六进制 ==========
fn sha1_hex(data: &[u8]) -> String {
    hex::encode(sha1::Sha1::digest(data))
}

/// 离线 UUID：与官方启动器一致 —— MD5("OfflinePlayer:" + 玩家名) 的 version 3 UUID
/// （Java UUID.nameUUIDFromBytes 行为；旧版用 SHA1 的算法与官方不一致）
fn offline_uuid(username: &str) -> String {
    let digest = md5::compute(format!("OfflinePlayer:{}", username).as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    let hex_str = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex_str[0..8], &hex_str[8..12], &hex_str[12..16], &hex_str[16..20], &hex_str[20..32]
    )
}

/// 离线玩家信息：UUID + 默认皮肤（1.21.5+ 官方 18 选 1 算法）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OfflineUuidInfo {
    uuid: String,
    /// 皮肤索引 0-17：0-8 slim 版 alex/ari/efe/kai/makena/noor/steve/sunny/zuri，
    /// 9-17 为同顺序 wide 版。与游戏内 DefaultPlayerSkin 完全一致。
    skin_index: u32,
    skin_name: String,
    /// slim=true 时玩家模型为瘦（Alex 型）
    slim: bool,
}

/// 根据玩家名计算离线 UUID 与默认皮肤（与游戏内选择逻辑一致）
#[tauri::command]
async fn get_offline_uuid(username: String) -> Result<OfflineUuidInfo, String> {
    if username.trim().is_empty() {
        return Err("玩家名不能为空".to_string());
    }
    let uuid = offline_uuid(username.trim());
    let undashed: String = uuid.chars().filter(|c| *c != '-').collect();

    // Java UUID.hashCode：hilo = msb ^ lsb；hash = (hilo >> 32) ^ hilo 的低 32 位
    let msb = u64::from_str_radix(&undashed[0..16], 16).unwrap_or(0);
    let lsb = u64::from_str_radix(&undashed[16..32], 16).unwrap_or(0);
    let hilo = msb ^ lsb;
    let hash = (((hilo >> 32) ^ hilo) & 0xFFFF_FFFF) as i64;
    // Java Math.floorMod(x, 18)
    let index = ((hash % 18) + 18) % 18;

    const SKIN_NAMES: [&str; 9] = ["alex", "ari", "efe", "kai", "makena", "noor", "steve", "sunny", "zuri"];
    let skin_name = SKIN_NAMES[(index % 9) as usize].to_string();
    let slim = index < 9;

    Ok(OfflineUuidInfo {
        uuid,
        skin_index: index as u32,
        skin_name,
        slim,
    })
}

// ========== 辅助：重试失败的资源下载 ==========
async fn retry_failed_assets(
    client: &reqwest::Client,
    objects: &serde_json::Map<String, serde_json::Value>,
    game_dir: &Path,
    mirrors: &Vec<&str>,
    window: &Window,
    cancel_flag: &Arc<AtomicBool>,
) -> usize {
    let mut failed_count = 0;
    let total = objects.len().max(1);
    let mut checked = 0usize;
    for (_key, value) in objects {
        let hash = match value["hash"].as_str() {
            Some(h) => h,
            None => continue,
        };
        let sub_path = format!("{}/{}", &hash[0..2], hash);
        let dest = game_dir.join("assets").join("objects").join(&sub_path);
        
        if dest.exists() {
            if let Ok(content) = fs::read(&dest) {
                let hash_calc = sha1_hex(&content);
                if hash_calc == hash {
                    checked += 1;
                    // 每校验 200 个文件更新一次进度（90 → 93 区间）
                    if checked % 200 == 0 {
                        let progress = 90 + (checked as u64 * 3 / total as u64).min(3);
                        let _ = window.emit("download-progress", json!({
                            "stage": format!("校验并补全游戏资源 ({}/{})", checked, total),
                            "progress": progress
                        }));
                    }
                    continue;
                }
            }
        }
        
        let url = format!("https://resources.download.minecraft.net/{}", sub_path);
        if let Err(e) = download_with_mirrors(client, &url, &dest, Some(hash), mirrors, window, cancel_flag, None).await {
            eprintln!("重试下载失败: {} - {}", url, e);
            failed_count += 1;
        }
        checked += 1;
        if checked % 200 == 0 {
            let progress = 90 + (checked as u64 * 3 / total as u64).min(3);
            let _ = window.emit("download-progress", json!({
                "stage": format!("校验并补全游戏资源 ({}/{})", checked, total),
                "progress": progress
            }));
        }
    }
    failed_count
}

// Minecraft 的版本 JSON 会用 rules 控制可选启动参数。例如 --demo 只应在
// is_demo_user 为 true 时出现。Lumia 不启用试玩、快速启动或自定义分辨率功能，
// 因此这些 feature 均为 false。
fn game_argument_allowed(rules: Option<&Vec<serde_json::Value>>) -> bool {
    let Some(rules) = rules else {
        return true;
    };

    let mut allowed = false;
    for rule in rules {
        let mut matches = true;

        if let Some(os) = rule["os"].as_object() {
            if let Some(name) = os.get("name").and_then(|value| value.as_str()) {
                matches &= match name {
                    "windows" => cfg!(target_os = "windows"),
                    "osx" => cfg!(target_os = "macos"),
                    "linux" => cfg!(target_os = "linux"),
                    "unknown" => true,
                    _ => false,
                };
            }
            if let Some(arch) = os.get("arch").and_then(|value| value.as_str()) {
                matches &= match arch {
                    "x86" => cfg!(target_arch = "x86_64"),
                    "arm64" => cfg!(target_arch = "aarch64"),
                    _ => true,
                };
            }
        }

        if let Some(features) = rule["features"].as_object() {
            // 当前启动器没有开启任何可选 feature；只允许明确要求 false 的规则。
            matches &= features.values().all(|value| value.as_bool() == Some(false));
        }

        if matches {
            allowed = rule["action"].as_str().unwrap_or("allow") == "allow";
        }
    }
    allowed
}

// ========== 辅助：获取 Java 路径 ==========
fn java_exe_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Windows 注册表读取已安装 JDK/JRE 的 JavaHome（无论装在哪个盘都能找到）
#[cfg(windows)]
fn java_from_registry() -> Option<String> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let root_keys = [
        r"SOFTWARE\JavaSoft\JDK",
        r"SOFTWARE\JavaSoft\Java Development Kit",
        r"SOFTWARE\JavaSoft\JRE",
    ];

    let mut homes: Vec<(String, Vec<i32>)> = Vec::new();
    for root in root_keys {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            if let Ok(rk) = hklm.open_subkey_with_flags(root, KEY_READ | view) {
                for subkey_name in rk.enum_keys().flatten() {
                    if let Ok(k) = rk.open_subkey(&subkey_name) {
                        if let Ok(home) = k.get_value::<String, _>("JavaHome") {
                            let java = PathBuf::from(&home).join("bin").join(java_exe_name());
                            if java.exists() {
                                let ver: Vec<i32> = subkey_name
                                    .split(['.', '_', '-'])
                                    .filter_map(|s| s.parse::<i32>().ok())
                                    .collect();
                                homes.push((java.to_str().unwrap().to_string(), ver));
                            }
                        }
                    }
                }
            }
        }
    }
    // 版本号大的优先（如 25 > 21 > 17 > 8）
    homes.sort_by(|a, b| {
        let mut ia = a.1.iter();
        let mut ib = b.1.iter();
        loop {
            match (ia.next(), ib.next()) {
                (None, None) => return std::cmp::Ordering::Equal,
                (None, Some(_)) => return std::cmp::Ordering::Less,
                (Some(_), None) => return std::cmp::Ordering::Greater,
                (Some(x), Some(y)) => match x.cmp(y) {
                    std::cmp::Ordering::Equal => continue,
                    other => return other,
                },
            }
        }
    });
    homes.into_iter().next().map(|(p, _)| p)
}

fn get_java_path_from_env() -> String {
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let java_path = PathBuf::from(&java_home).join("bin").join(java_exe_name());
        if java_path.exists() {
            return java_path.to_str().unwrap().to_string();
        }
    }
    detect_java_executable().unwrap_or_else(|| "java".to_string())
}

/// 自动探测已安装的 Java（PCL2 思路：候选目录深度搜索 + 环境变量 + 注册表）。
/// 参考 PCL2 PCLCS/Java.cs：候选文件夹（各启动器 runtime / .jdks / ProgramFiles 厂商目录）
/// 递归查找 bin/java.exe，加上 JAVA_HOME/JDK_HOME，注册表作为补充（可能装在任意盘）。
fn detect_java_executable() -> Option<String> {
    // 1. 环境变量
    for var in ["JAVA_HOME", "JDK_HOME"] {
        if let Ok(home) = std::env::var(var) {
            for part in home.split(';') {
                let p = PathBuf::from(part.trim().trim_matches('"'))
                    .join("bin")
                    .join(java_exe_name());
                if p.exists() {
                    return Some(p.to_str().unwrap().to_string());
                }
            }
        }
    }

    #[cfg(windows)]
    {
        if let Some(p) = java_from_registry() {
            return Some(p);
        }
    }

    // 2. 候选目录深度搜索（整盘根目录只搜 2 层，避免大目录耗时）
    let mut found: Vec<String> = Vec::new();
    for root in candidate_java_roots() {
        let is_drive_root = cfg!(windows) && root.as_os_str().to_string_lossy().ends_with('\\');
        let max_depth = if is_drive_root { 2 } else { 4 };
        find_java_recursive(&root, &mut found, 0, max_depth);
    }

    if found.is_empty() {
        return None;
    }
    found.sort_by(|a, b| version_from_path(b).cmp(&version_from_path(a)));
    found.into_iter().next()
}

/// 从路径中提取版本号数字序列（用于排序选最新）
fn version_from_path(path: &str) -> Vec<i32> {
    let mut nums: Vec<i32> = path
        .split(['\\', '/', '-', '_', '.', ' '])
        .filter_map(|s| s.parse::<i32>().ok())
        .filter(|&n| n != 32 && n != 64 && n != 0)
        .collect();
    nums.truncate(4);
    nums
}

/// 递归搜索目录下是否存在 bin/java(.exe)，最多深入 max_depth 层
fn find_java_recursive(dir: &Path, found: &mut Vec<String>, depth: usize, max_depth: usize) {
    if depth > max_depth {
        return;
    }
    let java = dir.join("bin").join(java_exe_name());
    if java.is_file() {
        found.push(java.to_str().unwrap().to_string());
        return;
    }
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // 跳过明显无关的大目录
            if depth == 0
                && matches!(
                    name.to_lowercase().as_str(),
                    "windows"
                        | "program files (x86)"
                        | "perflogs"
                        | "recovery"
                        | "system volume information"
                        | "$recycle.bin"
                        | "node_modules"
                )
            {
                continue;
            }
            find_java_recursive(&p, found, depth + 1, max_depth);
        }
    }
}

/// 候选目录列表（PCL2 同款 + 常见 D 盘位置）
fn candidate_java_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Ok(appdata) = std::env::var("APPDATA") {
        for sub in [
            r".minecraft\runtime",
            r".hmcl\java",
            r"ATLauncher\runtimes\minecraft",
            r"ModrinthApp\meta\java_versions",
            r"PrismLauncher\java",
        ] {
            roots.push(PathBuf::from(&appdata).join(sub));
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(&local).join(r".ftba\bin\runtime"));
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(PathBuf::from(&profile).join(".jdks"));
        roots.push(PathBuf::from(&profile).join(r".sdkman\candidates\java"));
        roots.push(PathBuf::from(&profile).join(r"curseforge\minecraft\Install\runtime"));
    }
    if cfg!(windows) {
        for drive in [
            r"C:\Program Files",
            r"C:\Program Files (x86)",
            r"D:\",
            r"D:\Program Files",
            r"D:\Java",
            r"D:\JDK",
            r"D:\Software",
            r"D:\Programs",
            r"D:\Tools",
            r"D:\Dev",
        ] {
            for sub in [
                "Java",
                "Eclipse Adoptium",
                "Amazon Corretto",
                "Zulu",
                "Microsoft",
                "jdk*",
                "JDK",
                "JavaSoft",
                r"Programs\Java",
                r"Software\Java",
                r"Apps\Java",
                r"Tools\Java",
                r"Dev\Java",
            ] {
                roots.push(PathBuf::from(drive).join(sub));
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        roots.push(PathBuf::from("/usr/local/opt"));
        roots.push(PathBuf::from("/opt/homebrew/opt"));
    } else {
        roots.push(PathBuf::from("/usr/lib/jvm"));
        roots.push(PathBuf::from("/opt"));
    }
    roots
}

// ========== 1. 获取在线版本列表 ==========
#[tauri::command]
async fn get_versions() -> Result<Vec<VersionManifestItem>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Lumia-Launcher/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))?;

    let endpoints = [
        "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json",
        "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json",
    ];
    let mut errors = Vec::new();

    for endpoint in endpoints {
        match client.get(endpoint).send().await {
            Ok(response) if response.status().is_success() => match response.json().await {
                Ok(json) => return parse_versions(json),
                Err(error) => errors.push(format!("{}：解析响应失败: {}", endpoint, error)),
            },
            Ok(response) => errors.push(format!("{}：HTTP {}", endpoint, response.status())),
            Err(error) => errors.push(format!("{}：{}", endpoint, error)),
        }
    }

    Err(format!("获取版本列表失败：{}", errors.join("；")))
}

fn is_april_fool(id: &str) -> bool {
    let april_fools = [
        "20w14infinite",
        "20w14∞",
        "3D Shareware v1.34",
        "oneblockatatime",
        "15w14a",
        "15w14b",
        "1.RV-Pre1",
        "1.RV-Pre2",
        "1.RV-Pre3",
        "22w13oneBlockAtATime",
        "25w14craftmine",
        "26w14a",
    ];
    april_fools.contains(&id) || id.contains("april") || id.contains("fool")
}

fn parse_versions(json: serde_json::Value) -> Result<Vec<VersionManifestItem>, String> {
    let versions = json["versions"]
        .as_array()
        .ok_or("无效的响应格式")?
        .iter()
        .filter_map(|version| {
            let id = version["id"].as_str()?;
            if id.contains("experimental") || id.contains("pending") {
                return None;
            }

            let version_type = version["type"].as_str().unwrap_or("unknown");
            let category = match version_type {
                "release" => "release",
                "snapshot" => {
                    if is_april_fool(id) {
                        "april_fool"
                    } else {
                        "snapshot"
                    }
                }
                "old_beta" | "old_alpha" => "ancient",
                _ => {
                    if is_april_fool(id) {
                        "april_fool"
                    } else if id.starts_with("b") || id.starts_with("a") || id.starts_with("indev") {
                        "ancient"
                    } else {
                        "unknown"
                    }
                }
            };

            Some(VersionManifestItem {
                id: id.to_string(),
                release_time: version["releaseTime"].as_str().unwrap_or_default().to_string(),
                category: category.to_string(),
            })
        })
        .collect::<Vec<VersionManifestItem>>();

    Ok(versions)
}

// ========== 本地版本信息 ==========
#[derive(Serialize)]
struct LocalVersionInfo {
    name: String,
    loader: String, // "vanilla" | "fabric" | "forge" | "neoforge"
}

// 根据版本 json 内容判断加载器类型（PCL2 做法：读 json 而非目录名）
fn detect_loader_from_json(json: &serde_json::Value) -> String {
    let json_str = serde_json::to_string(json).unwrap_or_default();
    if json_str.contains("net.fabricmc:fabric-loader") || json_str.contains("org.quiltmc:quilt-loader") {
        if json_str.contains("org.quiltmc:quilt-loader") {
            "quilt".to_string()
        } else {
            "fabric".to_string()
        }
    } else if json_str.contains("net.neoforged") {
        "neoforge".to_string()
    } else if json_str.contains("minecraftforge") || json_str.contains("net.minecraftforge") {
        "forge".to_string()
    } else {
        "vanilla".to_string()
    }
}

// ========== 2. 获取本地已安装版本 ==========
#[tauri::command]
async fn get_local_versions(app_handle: tauri::AppHandle) -> Result<Vec<LocalVersionInfo>, String> {
    let game_dir = get_game_dir(&app_handle).await;
    let versions_dir = game_dir.join("versions");

    if !versions_dir.exists() {
        return Ok(vec![]);
    }

    // 先收集所有版本及其加载器类型
    let mut all_versions: Vec<(String, String)> = vec![];
    for entry in fs::read_dir(&versions_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let json_path = path.join(format!("{}.json", name));
                if json_path.exists() {
                    if let Ok(json_str) = fs::read_to_string(&json_path) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                            let loader = detect_loader_from_json(&json);
                            all_versions.push((name.to_string(), loader));
                        }
                    }
                }
            }
        }
    }

    // 找出哪些版本是其他版本的 inheritsFrom 依赖
    let mut dependency_versions = std::collections::HashSet::new();
    for (ver_name, _) in &all_versions {
        let json_path = versions_dir.join(ver_name).join(format!("{}.json", ver_name));
        if let Ok(json_str) = fs::read_to_string(&json_path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                if let Some(inherits) = json["inheritsFrom"].as_str() {
                    dependency_versions.insert(inherits.to_string());
                }
            }
        }
    }

    // 过滤：只显示用户安装的版本
    // 1. Loader 版本（json 含 fabric/forge 等）始终显示
    // 2. 被 Loader 版本依赖的原版（inheritsFrom 指向它）→ 隐藏
    // 3. 独立安装的原版 → 显示
    let mut versions = vec![];
    for (ver_name, loader) in &all_versions {
        let is_loader = loader != "vanilla";
        let is_dependency = dependency_versions.contains(ver_name);
        
        if is_dependency && !is_loader {
            // 这个原版是某个 Loader 版本的父版本，隐藏避免重复
            continue;
        }
        versions.push(LocalVersionInfo {
            name: ver_name.clone(),
            loader: loader.clone(),
        });
    }

    Ok(versions)
}

// ========== 3. 下载游戏（多线程并发 + 国内源） ==========
#[tauri::command]
async fn cancel_download(app_handle: tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.cancel_download.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn download_game(
    window: Window,
    app_handle: tauri::AppHandle,
    config: DownloadConfig,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    state.cancel_download.store(false, Ordering::SeqCst);
    
    dl_log("download_game 命令已调用");
    dl_log(&format!("配置: 版本={}, 名称={}, Loader={}, Loader版本='{}', FabricAPI={}",
        config.version, config.display_name, config.loader, config.loader_version, config.install_fabric_api));
    
    // 最先发出进度事件：确认前端收到事件（这是代码构建标记，不是 Minecraft 版本号）
    let _ = window.emit("download-progress", json!({ "stage": "获取版本信息", "progress": 0 }));
    dl_log("已发出第一个进度事件: 获取版本信息");
    
    let version = config.version;
    let display_name = config.display_name;
    let loader = config.loader;
    let loader_version = config.loader_version;
    let install_fabric_api = config.install_fabric_api;

    if loader != "none" && loader_version.trim().is_empty() {
        return Err("请选择模组加载器版本后再下载".to_string());
    }
    
    eprintln!("========== 下载配置 ==========");
    eprintln!("版本: {}", version);
    eprintln!("显示名称: {}", display_name);
    eprintln!("Loader: {}", loader);
    eprintln!("Loader版本: '{}'", loader_version);
    eprintln!("安装Fabric API: {}", install_fabric_api);
    eprintln!("==============================");
    
    dl_log("获取游戏目录...");
    let _ = window.emit("download-progress", json!({ "stage": "获取游戏目录", "progress": 0 }));
    let game_dir = get_game_dir(&app_handle).await;
    dl_log(&format!("游戏目录: {:?}", game_dir));
    
    // 版本目录统一用原版版本名（加载器直接注入到原版 json，不创建独立目录）
    let (version_dir_name, final_version) = (version.clone(), version.clone());
    
    let versions_dir = game_dir.join("versions").join(&version_dir_name);
    let _ = window.emit("download-progress", json!({ "stage": "创建版本目录", "progress": 0 }));
    fs::create_dir_all(&versions_dir).map_err(|e| e.to_string())?;
    
    // 下载过程日志落盘到 versions/<版本>/downloads/lumia.log，出问题时可直接查看
    fs::create_dir_all(versions_dir.join("downloads")).map_err(|e| e.to_string())?;
    set_download_log_path(versions_dir.join("downloads").join("lumia.log"));
    dl_log(&format!("本次下载日志文件: {}", versions_dir.join("downloads").join("lumia.log").display()));
    
    // 创建版本目录的子文件夹结构
    for subdir in &["mods", "saves", "resourcepacks", "screenshots", "logs", "crash-reports", "shaderpacks"] {
        let _ = fs::create_dir_all(versions_dir.join(subdir));
    }
    
    eprintln!("版本目录: {:?}", versions_dir);
    eprintln!("最终版本名: {}", final_version);

    let mirrors = vec![
        "https://bmclapi2.bangbang93.com",
        "https://bmclapi.bangbang93.com",
    ];

    eprintln!("创建网络客户端...");
    let mut default_headers = reqwest::header::HeaderMap::new();
    default_headers.insert(
        reqwest::header::ACCEPT_ENCODING,
        reqwest::header::HeaderValue::from_static("identity"),
    );
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .default_headers(default_headers)
        .connect_timeout(std::time::Duration::from_secs(10))
        // 整个请求（含重定向、响应头）30 秒总超时，避免源不响应时无限挂起
        .timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .tcp_nodelay(true)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))?;

    let client = Arc::new(client);
    let cancel_flag = state.cancel_download.clone();

    // 获取版本清单。元数据请求使用独立短超时，避免网络异常时任务长期停留在“准备下载”。
    let _ = window.emit("download-progress", json!({ "stage": "获取版本清单", "progress": 1 }));
    dl_log("开始获取版本清单...");
    let mut manifest_opt = None;
    let mut last_mirror_error = String::new();
    for (idx, mirror) in mirrors.iter().enumerate() {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }
        
        let url = format!("{}/mc/game/version_manifest_v2.json", mirror);
        dl_log(&format!("尝试镜像源 ({}): {}", idx + 1, url));
        let _ = window.emit("download-progress", json!({
            "stage": format!("获取版本清单 (源 {}/{})", idx + 1, mirrors.len()),
            "progress": 1
        }));
        match tokio::time::timeout(Duration::from_secs(15), client.get(&url).send()).await {
            Err(_) => { eprintln!("镜像源 {} 请求超时", mirror); last_mirror_error = format!("镜像源 {} 请求超时", mirror); }
            Ok(Err(e)) => { eprintln!("镜像源 {} 请求失败: {}", mirror, e); last_mirror_error = format!("镜像源 {} 请求失败: {}", mirror, e); }
            Ok(Ok(response)) if response.status().is_success() => {
                dl_log(&format!("镜像源 {} 请求成功", mirror));
                match tokio::time::timeout(Duration::from_secs(10), response.json()).await {
                    Ok(Ok(json)) => {
                        dl_log("解析版本清单成功");
                        manifest_opt = Some(json);
                        break;
                    }
                    Ok(Err(e)) => { eprintln!("镜像源 {} 清单解析失败: {}", mirror, e); last_mirror_error = format!("镜像源 {} 清单解析失败: {}", mirror, e); }
                    Err(_) => { eprintln!("镜像源 {} 清单解析超时", mirror); last_mirror_error = format!("镜像源 {} 清单解析超时", mirror); }
                }
            }
            Ok(Ok(response)) => { eprintln!("镜像源 {} 返回状态码: {}", mirror, response.status()); last_mirror_error = format!("镜像源 {} 返回状态码: {}", mirror, response.status()); }
        }
    }
    if manifest_opt.is_none() {
        let url = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
        eprintln!("尝试原始 URL: {}", url);
        let _ = window.emit("download-progress", json!({ "stage": "获取版本清单 (官方源)", "progress": 1 }));
        match tokio::time::timeout(Duration::from_secs(15), client.get(url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => match tokio::time::timeout(Duration::from_secs(10), response.json()).await {
                Ok(Ok(json)) => {
                    eprintln!("原始 URL 解析版本清单成功");
                    manifest_opt = Some(json);
                }
                Ok(Err(e)) => eprintln!("原始 URL 清单解析失败: {}", e),
                Err(_) => eprintln!("原始 URL 清单解析超时"),
            },
            Ok(Ok(response)) => eprintln!("原始 URL 返回状态码: {}", response.status()),
            Ok(Err(e)) => eprintln!("原始 URL 请求失败: {}", e),
            Err(_) => eprintln!("原始 URL 请求超时"),
        }
    }
    eprintln!("版本清单获取完成");
    let manifest: serde_json::Value = manifest_opt
        .ok_or_else(|| format!("所有镜像源均无法获取版本清单：{}", last_mirror_error))?;

    let version_url = manifest["versions"]
        .as_array()
        .ok_or("无效的版本清单")?
        .iter()
        .find(|v| v["id"].as_str().unwrap_or("") == version)
        .ok_or(format!("未找到版本: {}", version))?
        ["url"]
        .as_str()
        .ok_or("版本 URL 缺失")?;

    // 获取版本详情
    let _ = window.emit("download-progress", json!({ "stage": "获取版本详情", "progress": 3 }));
    let mut version_opt = None;
    let mut version_detail_err = String::new();
    for (idx, mirror) in mirrors.iter().enumerate() {
        // version_url 实际指向 piston-meta.mojang.com，旧代码只替换 launchermeta 导致永远走官方源
        let url = version_url
            .replace("https://launchermeta.mojang.com", mirror)
            .replace("https://piston-meta.mojang.com", mirror)
            .replace("https://piston-data.mojang.com", mirror);
        dl_log(&format!("获取版本详情，尝试镜像源 ({}): {}", idx + 1, url));
        match tokio::time::timeout(Duration::from_secs(15), client.get(&url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => match tokio::time::timeout(Duration::from_secs(10), response.json()).await {
                Ok(Ok(json)) => {
                    dl_log(&format!("版本详情获取成功，来源镜像: {}", mirror));
                    version_opt = Some(json);
                    break;
                }
                Ok(Err(e)) => { version_detail_err = format!("镜像 {} 版本详情解析失败: {}", mirror, e); }
                Err(_) => { version_detail_err = format!("镜像 {} 版本详情解析超时", mirror); }
            },
            Ok(Ok(response)) => { version_detail_err = format!("镜像 {} 版本详情返回状态码: {}", mirror, response.status()); }
            Ok(Err(e)) => { version_detail_err = format!("镜像 {} 版本详情请求失败: {}", mirror, e); }
            Err(_) => { version_detail_err = format!("镜像 {} 版本详情请求超时", mirror); }
        }
    }
    if version_opt.is_none() {
        dl_log("镜像获取版本详情均失败，尝试官方原始 URL");
        match tokio::time::timeout(Duration::from_secs(15), client.get(version_url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => {
                if let Ok(Ok(json)) = tokio::time::timeout(Duration::from_secs(10), response.json()).await {
                    version_opt = Some(json);
                    dl_log("官方原始 URL 获取版本详情成功");
                } else {
                    version_detail_err = format!("官方源版本详情解析失败/超时 ({})", version_url);
                }
            }
            Ok(Ok(response)) => version_detail_err = format!("官方源版本详情返回状态码 {} ({})", response.status(), version_url),
            Ok(Err(e)) => version_detail_err = format!("官方源版本详情请求失败: {} ({})", e, version_url),
            Err(_) => version_detail_err = format!("官方源版本详情请求超时 ({})", version_url),
        }
    }
    let version_json: serde_json::Value = version_opt.ok_or(format!(
        "所有镜像源均无法获取版本详情：{}",
        version_detail_err
    ))?;

    // 保存版本 JSON
    let json_path = versions_dir.join(format!("{}.json", version_dir_name));
    
    // Fabric 使用启动器生成的 profile；Forge/NeoForge 必须交由各自安装器生成，
    // 否则会留下只继承原版的伪 profile，导致启动时回退到原版。
    let loader_uses_installer_profile = loader == "forge" || loader == "neoforge";
    let mut version_json_to_save = version_json.clone();
    if loader != "none" && !loader_uses_installer_profile {
        // 注入模式：不设 inheritsFrom（版本目录就是原版目录），加载器内容直接写入
        // modify_version_json_for_fabric 会补充 mainClass / libraries / arguments
        version_json_to_save["id"] = serde_json::json!(version_dir_name);
        if let Some(downloads) = version_json_to_save["downloads"].as_object_mut() {
            downloads.remove("client");
        }
    }

    // 无论是否使用安装器，都先保存原版 json：Forge/NeoForge 安装器需要读取它
    // 才能生成 profile；安装完成后会由注入逻辑覆盖为加载器版本。
    fs::write(
        &json_path,
        serde_json::to_string_pretty(&version_json_to_save).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    // 下载客户端 jar（原版或加载器版本都在当前版本目录）
    {
        let _ = window.emit("download-progress", json!({ "stage": "下载客户端", "progress": 10 }));

        let mut client_url = version_json["downloads"]["client"]["url"]
            .as_str()
            .ok_or("客户端下载 URL 缺失")?
            .to_string();
        // 必须带 sha1 校验：CDN 截断的坏 jar 不能当成成功保存
        let client_sha1 = version_json["downloads"]["client"]["sha1"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let expected_sha1: Option<&str> = if client_sha1.is_empty() { None } else { Some(&client_sha1) };

        // 客户端 jar 走官方优先：一旦拿到 sha1，就用官方固定路径 piston-data.mojang.com 作为首选源，
        // 避免版本详情 json 来自镜像（BMCLAPI）时把 client url 改写成 bmclapi，导致绕过官方源而卡死。
        // download_with_mirrors 会先用官方 URL，失败再切 bmclapi 镜像兜底。
        if !client_sha1.is_empty() {
            let official_url = format!(
                "https://piston-data.mojang.com/v1/objects/{}/client.jar",
                client_sha1
            );
            if !client_url.contains("mojang.com") {
                dl_log(&format!("客户端 jar 镜像 URL: {} → 改用官方 URL: {}", client_url, official_url));
                client_url = official_url;
            }
        }

        let jar_path = versions_dir.join(format!("{}.jar", version_dir_name));
        download_with_mirrors(&client, &client_url, &jar_path, expected_sha1, &mirrors, &window, &cancel_flag, Some((10.0, 19.0))).await?;
    }

    // 下载 Libraries（并发）
    let _ = window.emit("download-progress", json!({ "stage": "下载依赖库", "progress": 20 }));

    let libraries = version_json["libraries"].as_array().ok_or("无效的 libraries 格式")?;
    let total_libs = libraries.len();
    let lib_done = Arc::new(AtomicUsize::new(0));
    let lib_sem = Arc::new(Semaphore::new(32));
    // 收集失败的库（url, 目标路径），下载完成后统一重试
    let lib_failures: Arc<Mutex<Vec<(String, PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));

    let mut lib_tasks = JoinSet::new();

    for lib in libraries {
        // 方式1: 有 downloads.artifact 的标准格式
        if let Some(artifact) = lib["downloads"]["artifact"].as_object() {
            if let Some(path) = artifact["path"].as_str() {
                let url = artifact["url"].as_str().unwrap_or("").to_string();
                let sha1_val = artifact["sha1"].as_str().unwrap_or("").to_string();
                let lib_path = game_dir.join("libraries").join(path);

                if lib_path.exists() && !sha1_val.is_empty() {
                    if let Ok(content) = fs::read(&lib_path) {
                        let hash = sha1_hex(&content);
                        if hash == sha1_val {
                            lib_done.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    }
                }

                let client_clone = client.clone();
                let mirrors_clone = mirrors.clone();
                let window_clone = window.clone();
                let sem = lib_sem.clone();
                let done = lib_done.clone();
                let failures = lib_failures.clone();

                let cancel_flag_clone = cancel_flag.clone();
                lib_tasks.spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    if cancel_flag_clone.load(Ordering::SeqCst) {
                        done.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    if !url.is_empty() {
                        if let Err(e) = download_with_mirrors(&client_clone, &url, &lib_path, Some(&sha1_val), &mirrors_clone, &window_clone, &cancel_flag_clone, None).await {
                            eprintln!("库下载失败: {} - {}", url, e);
                            failures.lock().await.push((url, lib_path, sha1_val));
                        }
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                });
            } else {
                // artifact 存在但 path 为空，跳过
                lib_done.fetch_add(1, Ordering::Relaxed);
            }
        }
        // 方式2: 只有 name 字段的格式 (Fabric/Forge loader libraries)
        else if let Some(name) = lib["name"].as_str() {
            let parts: Vec<&str> = name.split(':').collect();
            if parts.len() >= 3 {
                let group_path = parts[0].replace('.', "/");
                let artifact = parts[1];
                let lib_version = parts[2];
                let file_name = if parts.len() == 4 {
                    format!("{}-{}-{}.jar", artifact, lib_version, parts[3])
                } else {
                    format!("{}-{}.jar", artifact, lib_version)
                };
                let lib_path_str = format!("{}/{}/{}/{}", group_path, artifact, lib_version, file_name);
                let lib_path = game_dir.join("libraries").join(&lib_path_str);
                
                let url = if let Some(lib_url) = lib["url"].as_str() {
                    format!("{}{}", lib_url, lib_path_str)
                } else {
                    format!("https://maven.fabricmc.net/{}", lib_path_str)
                };

                if lib_path.exists() {
                    lib_done.fetch_add(1, Ordering::Relaxed);
                    continue;
                }

                let client_clone = client.clone();
                let mirrors_clone = mirrors.clone();
                let window_clone = window.clone();
                let sem = lib_sem.clone();
                let done = lib_done.clone();
                let failures = lib_failures.clone();

                let cancel_flag_clone = cancel_flag.clone();
                lib_tasks.spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    if cancel_flag_clone.load(Ordering::SeqCst) {
                        done.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    if let Err(e) = download_with_mirrors(&client_clone, &url, &lib_path, None, &mirrors_clone, &window_clone, &cancel_flag_clone, None).await {
                        eprintln!("库下载失败: {} - {}", url, e);
                        failures.lock().await.push((url, lib_path, String::new()));
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                });
            } else {
                lib_done.fetch_add(1, Ordering::Relaxed);
            }
        }
        else {
            lib_done.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    eprintln!("库下载任务已创建: {} 个, 总数: {}", lib_tasks.len(), total_libs);

    // 轮询更新进度
    let total_libs_f = total_libs;
    let lib_done_f = lib_done.clone();
    let window_clone = window.clone();
    let cancel_flag_clone_prog = cancel_flag.clone();
    tokio::spawn(async move {
        let mut last_done = 0;
        while lib_done_f.load(Ordering::Relaxed) < total_libs_f && !cancel_flag_clone_prog.load(Ordering::SeqCst) {
            let done = lib_done_f.load(Ordering::Relaxed);
            if done != last_done {
                let progress = 20 + (done * 30 / total_libs_f);
                let _ = window_clone.emit("download-progress", json!({
                    "stage": format!("下载库 ({}/{})", done, total_libs_f),
                    "progress": progress.min(50)
                }));
                last_done = done;
            }
            sleep(Duration::from_millis(500)).await;
        }
        if !cancel_flag_clone_prog.load(Ordering::SeqCst) {
            let _ = window_clone.emit("download-progress", json!({
                "stage": "依赖库下载完成",
                "progress": 50
            }));
        }
    });

    // 等待所有库下载完成或被取消
    while lib_done.load(Ordering::Relaxed) < total_libs {
        if cancel_flag.load(Ordering::SeqCst) {
            eprintln!("下载已取消，终止等待库下载");
            return Err("下载已取消".to_string());
        }
        sleep(Duration::from_millis(100)).await;
    }
    while let Some(_) = lib_tasks.join_next().await {}
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    // 重试失败的库（并发，避免逐个串行重试太慢）
    let failures = {
        let f = lib_failures.lock().await;
        f.clone()
    };
    if !failures.is_empty() {
        eprintln!("有 {} 个库下载失败，并发重试...", failures.len());
        let retry_sem = Arc::new(Semaphore::new(16));
        let mut retry_tasks = JoinSet::new();
        for (url, lib_path, sha1_val) in &failures {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("下载已取消".to_string());
            }
            let sem = retry_sem.clone();
            let client_c = client.clone();
            let mirrors_c = mirrors.clone();
            let window_c = window.clone();
            let cancel_c = cancel_flag.clone();
            let url_c = url.clone();
            let path_c = lib_path.clone();
            let sha1_c = sha1_val.clone();
            retry_tasks.spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                if cancel_c.load(Ordering::SeqCst) {
                    return;
                }
                let expected = if sha1_c.is_empty() { None } else { Some(sha1_c.as_str()) };
                if let Err(e) = download_with_mirrors(&client_c, &url_c, &path_c, expected, &mirrors_c, &window_c, &cancel_c, None).await {
                    eprintln!("重试仍失败: {} - {}", url_c, e);
                }
            });
        }
        while let Some(_) = retry_tasks.join_next().await {}
        let still_missing: Vec<_> = failures.iter().filter(|(_, p, _)| !p.exists()).collect();
        if !still_missing.is_empty() {
            return Err(format!(
                "仍有 {} 个依赖库无法下载，已停止安装以避免启动资源缺失，请检查网络后重试",
                still_missing.len()
            ));
        }
    }

    let _ = window.emit("download-progress", json!({ "stage": "依赖库下载完成", "progress": 50 }));

    // 下载 Assets（并发）
    let _ = window.emit("download-progress", json!({ "stage": "下载资源文件", "progress": 50 }));

    let asset_index = version_json["assetIndex"].as_object().ok_or("缺少 assetIndex")?;
    let asset_index_url = asset_index["url"].as_str().ok_or("缺少 assetIndex url")?;
    let asset_index_id = asset_index["id"].as_str().unwrap_or("legacy");

    let mut index_bytes_opt = None;
    // 官方优先 + 镜像兜底，每个请求带超时，防止 bmclapi 不可用时卡死
    let mut index_sources: Vec<String> = vec![asset_index_url.to_string()];
    for mirror in &mirrors {
        let url = asset_index_url
            .replace("https://launchermeta.mojang.com", mirror)
            .replace("https://piston-meta.mojang.com", mirror)
            .replace("https://piston-data.mojang.com", mirror);
        if url != asset_index_url && !index_sources.contains(&url) {
            index_sources.push(url);
        }
    }
    let mut last_index_err = String::new();
    for url in &index_sources {
        match tokio::time::timeout(Duration::from_secs(15), client.get(url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => {
                match tokio::time::timeout(Duration::from_secs(10), response.bytes()).await {
                    Ok(Ok(bytes)) => {
                        index_bytes_opt = Some(bytes);
                        break;
                    }
                    Ok(Err(e)) => { last_index_err = format!("读取资源索引失败: {}", e); }
                    Err(_) => { last_index_err = format!("读取资源索引超时: {}", url); }
                }
            }
            Ok(Ok(response)) => { last_index_err = format!("资源索引 HTTP {}: {}", response.status(), url); }
            Ok(Err(e)) => { last_index_err = format!("资源索引请求失败: {} - {}", url, e); }
            Err(_) => { last_index_err = format!("资源索引请求超时: {}", url); }
        }
    }
    let index_bytes = index_bytes_opt.ok_or_else(|| format!("无法下载资源索引：{}", last_index_err))?;

    let index_json: serde_json::Value = serde_json::from_slice(&index_bytes)
        .map_err(|e| format!("解析资源索引失败: {}", e))?;

    let indexes_dir = game_dir.join("assets").join("indexes");
    fs::create_dir_all(&indexes_dir).map_err(|e| e.to_string())?;
    let index_path = indexes_dir.join(format!("{}.json", asset_index_id));
    fs::write(&index_path, &index_bytes).map_err(|e| e.to_string())?;

    let objects = index_json["objects"].as_object().ok_or("无效的资源索引格式")?;
    let total_objects = objects.len();
    let asset_done = Arc::new(AtomicUsize::new(0));
    let asset_errors = Arc::new(AtomicUsize::new(0));
    let asset_sem = Arc::new(Semaphore::new(64));

    let mut asset_tasks = JoinSet::new();

    for (_key, value) in objects.iter() {
        let hash = value["hash"].as_str().ok_or("缺少 hash")?.to_string();
        let sub_path = format!("{}/{}", &hash[0..2], hash);
        let url = format!("https://resources.download.minecraft.net/{}", sub_path);
        let dest = game_dir.join("assets").join("objects").join(&sub_path);

        if dest.exists() {
            if let Ok(content) = fs::read(&dest) {
                let hash_calc = sha1_hex(&content);
                if hash_calc == hash {
                    asset_done.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            }
        }

        let client_clone = client.clone();
        let mirrors_clone = mirrors.clone();
        let window_clone = window.clone();
        let sem = asset_sem.clone();
        let done = asset_done.clone();
        let errors = asset_errors.clone();

        let cancel_flag_clone = cancel_flag.clone();
        asset_tasks.spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            if cancel_flag_clone.load(Ordering::SeqCst) {
                done.fetch_add(1, Ordering::Relaxed);
                return;
            }
            if let Err(e) = download_with_mirrors(&client_clone, &url, &dest, Some(&hash), &mirrors_clone, &window_clone, &cancel_flag_clone, None).await {
                eprintln!("资源下载失败: {} - {}", url, e);
                errors.fetch_add(1, Ordering::Relaxed);
            }
            done.fetch_add(1, Ordering::Relaxed);
        });
    }

    let total_assets_f = total_objects;
    let asset_done_f = asset_done.clone();
    let window_clone = window.clone();
    let cancel_flag_clone_assets = cancel_flag.clone();
    tokio::spawn(async move {
        let mut last_done = 0;
        while asset_done_f.load(Ordering::Relaxed) < total_assets_f && !cancel_flag_clone_assets.load(Ordering::SeqCst) {
            let done = asset_done_f.load(Ordering::Relaxed);
            if done != last_done {
                let progress = 50 + (done * 40 / total_assets_f);
                let _ = window_clone.emit("download-progress", json!({
                    "stage": format!("下载资源 ({}/{})", done, total_assets_f),
                    "progress": progress.min(90)
                }));
                last_done = done;
            }
            sleep(Duration::from_millis(500)).await;
        }
        // 收尾发射交给主流程，避免与后续阶段（90→93）产生竞态倒退
    });

    // 等待所有资源下载完成或被取消
    while asset_done.load(Ordering::Relaxed) < total_objects {
        if cancel_flag.load(Ordering::SeqCst) {
            eprintln!("下载已取消，终止等待资源下载");
            return Err("下载已取消".to_string());
        }
        sleep(Duration::from_millis(100)).await;
    }
    while let Some(_) = asset_tasks.join_next().await {}
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    // 下载线程结束后，以资源索引逐个补全并校验。不能让任何缺失资源进入
    // “下载完成”状态，否则带加载器的版本会在游戏内表现为纹理/语言资源缺失。
    let error_count = asset_errors.load(Ordering::SeqCst);
    if error_count > 0 {
        eprintln!("检测到 {} 个资源下载失败，开始补全校验", error_count);
    }
    let _ = window.emit("download-progress", json!({ "stage": "校验并补全游戏资源", "progress": 90 }));
    let retry_count = retry_failed_assets(&client, &objects, &game_dir, &mirrors, &window, &cancel_flag).await;
    if retry_count > 0 {
        return Err(format!(
            "{} 个游戏资源在补全后仍无法下载，已停止安装以避免资源缺失，请检查网络后重试",
            retry_count
        ));
    }

    let _ = window.emit("download-progress", json!({ "stage": "资源校验完成", "progress": 93 }));

    // 下载 natives
    let _ = window.emit("download-progress", json!({ "stage": "下载本地库 (natives)", "progress": 94 }));

    let os_name = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err("不支持的操作系统".to_string());
    };

    let classifier_key = match os_name {
        "windows" => "natives-windows",
        "osx" => "natives-osx",
        "linux" => "natives-linux",
        _ => return Err("不支持的操作系统".to_string()),
    };

    // 收集需要提取 natives 的 libraries（包括当前版本和父版本）
    let mut libs_to_process = Vec::new();
    
    if let Some(libraries) = version_json["libraries"].as_array() {
        for lib in libraries {
            if lib["downloads"]["classifiers"].as_object().is_some() {
                libs_to_process.push(lib.clone());
            }
        }
    }
    
    // 对于加载器版本，也从父版本的 libraries 中提取 natives
    if loader != "none" {
        let parent_json_path = game_dir.join("versions").join(&version).join(format!("{}.json", version));
        if parent_json_path.exists() {
            if let Ok(parent_json_str) = fs::read_to_string(&parent_json_path) {
                if let Ok(parent_json) = serde_json::from_str::<serde_json::Value>(&parent_json_str) {
                    if let Some(parent_libraries) = parent_json["libraries"].as_array() {
                        for lib in parent_libraries {
                            if lib["downloads"]["classifiers"].as_object().is_some() {
                                libs_to_process.push(lib.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    for lib in &libs_to_process {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }
        if let Some(classifiers) = lib["downloads"]["classifiers"].as_object() {
            if let Some(native) = classifiers.get(classifier_key) {
                if let Some(url) = native["url"].as_str() {
                    let path = native["path"].as_str().unwrap_or("");
                    let sha1_val = native["sha1"].as_str().unwrap_or("");
                    let dest = game_dir.join("libraries").join(path);
                    if !dest.exists() || (sha1_val.is_empty() && dest.exists()) {
                        download_with_mirrors(&client, url, &dest, Some(sha1_val), &mirrors, &window, &cancel_flag, None).await?;
                    }
                    let natives_dir = versions_dir.join("natives");
                    if !natives_dir.exists() {
                        fs::create_dir_all(&natives_dir).map_err(|e| e.to_string())?;
                    }
                    let file = fs::File::open(&dest).map_err(|e| e.to_string())?;
                    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
                    for i in 0..archive.len() {
                        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
                        let outpath = natives_dir.join(file.name());
                        if file.is_dir() {
                            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
                        } else {
                            if let Some(parent) = outpath.parent() {
                                if !parent.exists() {
                                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                                }
                            }
                            let mut outfile = fs::File::create(&outpath).map_err(|e| e.to_string())?;
                            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
                        }
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if let Some(perm) = file.unix_mode() {
                                fs::set_permissions(&outpath, fs::Permissions::from_mode(perm))
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                    }
                }
            }
        }
    }
    
    // 对于加载器版本，也为父版本提取 natives
    if loader != "none" {
        let parent_natives_dir = game_dir.join("versions").join(&version).join("natives");
        if !parent_natives_dir.exists() {
            fs::create_dir_all(&parent_natives_dir).map_err(|e| e.to_string())?;
            
            // 从父版本的 libraries 中提取 natives
            let parent_json_path = game_dir.join("versions").join(&version).join(format!("{}.json", version));
            if parent_json_path.exists() {
                if let Ok(parent_json_str) = fs::read_to_string(&parent_json_path) {
                    if let Ok(parent_json) = serde_json::from_str::<serde_json::Value>(&parent_json_str) {
                        if let Some(parent_libraries) = parent_json["libraries"].as_array() {
                            for lib in parent_libraries {
                                if let Some(classifiers) = lib["downloads"]["classifiers"].as_object() {
                                    if let Some(native) = classifiers.get(classifier_key) {
                                        if let Some(path) = native["path"].as_str() {
                                            let dest = game_dir.join("libraries").join(path);
                                            if dest.exists() {
                                                let file = fs::File::open(&dest).map_err(|e| e.to_string())?;
                                                if let Ok(mut archive) = zip::ZipArchive::new(file) {
                                                    for i in 0..archive.len() {
                                                        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
                                                        let outpath = parent_natives_dir.join(file.name());
                                                        if file.is_dir() {
                                                            fs::create_dir_all(&outpath).map_err(|e| e.to_string())?;
                                                        } else {
                                                            if let Some(parent) = outpath.parent() {
                                                                if !parent.exists() {
                                                                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                                                                }
                                                            }
                                                            let mut outfile = fs::File::create(&outpath).map_err(|e| e.to_string())?;
                                                            let _ = std::io::copy(&mut file, &mut outfile);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 安装 Loader（Fabric/Forge/NeoForge）
    if loader != "none" {
        let _ = window.emit("download-progress", json!({ "stage": format!("安装 {} Loader", loader), "progress": 95 }));
        
        let version_dir_name = versions_dir.file_name().unwrap().to_str().unwrap_or(&version);
        
        match loader.as_str() {
            "fabric" => {
                install_fabric_loader(&client, &version, &version_dir_name, &game_dir, &versions_dir, &window, install_fabric_api, &loader_version, &cancel_flag).await?;
            }
            "forge" => {
                install_forge_loader(&client, &version, &version_dir_name, &game_dir, &versions_dir, &window, &loader_version, &cancel_flag).await?;
            }
            "neoforge" => {
                install_neoforge_loader(&client, &version, &version_dir_name, &game_dir, &versions_dir, &mirrors, &window, &loader_version, &cancel_flag).await?;
            }
            _ => {}
        }

        if !json_path.exists() {
            return Err(format!("{} 安装器未生成可启动的版本配置：{}", loader, json_path.display()));
        }
    }

    let _ = window.emit("download-progress", json!({ "stage": "下载完成", "progress": 100 }));

    Ok(())
}

// ========== 辅助：使用镜像源下载文件 ==========
// progress_range: Some((start, end)) 时，在下载过程中按字节数发射进度事件（用于大文件串行下载，如客户端 jar）
async fn download_with_mirrors(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    mirrors: &Vec<&str>,
    window: &Window,
    cancel_flag: &Arc<AtomicBool>,
    progress_range: Option<(f64, f64)>,
) -> Result<(), String> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    if dest.exists() {
        if let Some(sha1) = expected_sha1 {
            if let Ok(content) = fs::read(dest) {
                let hash = sha1_hex(&content);
                if hash == sha1 {
                    return Ok(());
                }
            }
            // sha1 不匹配：坏文件，删除后重新下载
            let _ = fs::remove_file(dest);
        } else {
            // 无 sha1 校验：文件存在就跳过，但过小的文件（<1KB）视为损坏残留，重新下载
            if let Ok(meta) = fs::metadata(dest) {
                if meta.len() > 1024 {
                    return Ok(());
                }
            }
            let _ = fs::remove_file(dest);
        }
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let max_retries = 2;
    // 单次下载 45 秒无数据即超时，避免卡死在慢速源
    let chunk_timeout = Duration::from_secs(45);

    // ========== 策略：官方优先，镜像兜底（PCL2 默认行为） ==========
    // 实测：当前网络下官方源最稳定（大文件完整），bmclapi 大文件截断/超时严重。
    // 先试官方，失败再试镜像，避免浪费大量时间在不可用的镜像上。
    let mut mirror_urls: Vec<String> = Vec::new();
    for mirror in mirrors {
        let replaced = url
            .replace("https://launcher.mojang.com", mirror)
            .replace("https://piston-data.mojang.com", mirror)
            .replace("https://piston-meta.mojang.com", mirror)
            .replace("https://launchermeta.mojang.com", mirror)
            .replace("https://libraries.minecraft.net", "https://mirror.nju.edu.cn/bmclapi")
            .replace("https://resources.download.minecraft.net", &format!("{}/assets", mirror))
            .replace("https://client-download.mojang.com", &format!("{}/client", mirror))
            .replace("https://maven.fabricmc.net", "https://mirror.nju.edu.cn/bmclapi")
            .replace("https://maven.minecraftforge.net", "https://mirror.nju.edu.cn/bmclapi")
            .replace("https://files.minecraftforge.net", "https://mirror.nju.edu.cn/bmclapi")
            // NeoForge maven 带 /releases 或 /snapshots 仓库段：镜像端对应 /maven 路径
            .replace("https://maven.neoforged.net/releases", &format!("{}/maven", mirror))
            .replace("https://maven.neoforged.net/snapshots", &format!("{}/maven", mirror))
            .replace("https://maven.neoforged.net", "https://mirror.nju.edu.cn/bmclapi");
        if replaced != url && !mirror_urls.contains(&replaced) {
            mirror_urls.push(replaced);
        }
    }

    // 官方原始 URL 放最前面，镜像放后面兜底
    let mut sources: Vec<String> = vec![url.to_string()];
    for m in mirror_urls {
        if !sources.contains(&m) {
            sources.push(m);
        }
    }

    let total_sources = sources.len();
    for (source_idx, source_url) in sources.iter().enumerate() {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }

        for attempt in 0..max_retries {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("下载已取消".to_string());
            }
            if attempt > 0 {
                sleep(Duration::from_millis(200)).await;
            }

            let source_tag = if source_idx == 0 { "原始" } else { "镜像" };
            // send 加 25 秒超时（client 总超时 30 秒），防止源不响应时无限挂起
            let response = match tokio::time::timeout(Duration::from_secs(25), client.get(source_url).send()).await {
                Ok(Ok(resp)) => resp,
                Ok(Err(e)) => {
                    eprintln!("{}源请求失败 ({}): {}", source_tag, source_url, e);
                    continue;
                }
                Err(_) => {
                    eprintln!("{}源请求超时 (25s): {}", source_tag, source_url);
                    continue;
                }
            };

            if !response.status().is_success() {
                eprintln!("{}源 HTTP 错误 ({}): {} - {}", source_tag, attempt + 1, response.status(), source_url);
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    break;
                }
                continue;
            }

            let temp_path = dest.with_extension("tmp");
            let mut file = match std::fs::File::create(&temp_path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("创建临时文件失败: {}", e);
                    continue;
                }
            };

            let total_bytes = response.content_length().unwrap_or(0);
            let mut downloaded_bytes: u64 = 0;
            let mut stream = response.bytes_stream();
            let mut hasher = sha1::Sha1::new();
            let mut stream_ok = true;
            let mut last_emit_percent: i64 = -1;

            loop {
                if cancel_flag.load(Ordering::SeqCst) {
                    eprintln!("下载被取消");
                    stream_ok = false;
                    break;
                }
                match tokio::time::timeout(chunk_timeout, stream.next()).await {
                    Ok(Some(chunk_result)) => match chunk_result {
                        Ok(chunk) => {
                            downloaded_bytes += chunk.len() as u64;
                            hasher.update(&chunk);
                            if let Err(e) = file.write_all(&chunk) {
                                eprintln!("写入失败: {}", e);
                                stream_ok = false;
                                break;
                            }
                            // 大文件按字节发射进度（每 1% 一次，节流）
                            if let Some((p_start, p_end)) = progress_range {
                                if total_bytes > 0 {
                                    let percent = ((downloaded_bytes as f64 / total_bytes as f64) * 100.0) as i64;
                                    if percent != last_emit_percent {
                                        last_emit_percent = percent;
                                        let progress = p_start + (p_end - p_start) * (downloaded_bytes as f64 / total_bytes as f64);
                                        let _ = window.emit("download-progress", json!({
                                            "stage": "下载客户端",
                                            "progress": progress.round() as u64,
                                            "downloaded": downloaded_bytes,
                                            "total": total_bytes,
                                        }));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("读取失败: {} (已下载 {} / {} 字节, 源 {})", e, downloaded_bytes, total_bytes, source_url);
                            stream_ok = false;
                            break;
                        }
                    },
                    Ok(None) => break,
                    Err(_) => {
                        eprintln!("下载超时: {} 秒无数据", chunk_timeout.as_secs());
                        stream_ok = false;
                        break;
                    }
                }
            }

            if !stream_ok {
                let _ = std::fs::remove_file(&temp_path);
                // 流读取失败（解码错误/截断）：CDN 截断是偶发的，同源重试大概率成功
                continue;
            }

            // Content-Length 完整性校验：服务器声明了长度就必须完全匹配，防止截断文件被保存
            if total_bytes > 0 && downloaded_bytes != total_bytes {
                let _ = std::fs::remove_file(&temp_path);
                eprintln!("文件不完整: 期望 {} 字节, 实际 {} 字节 ({})", total_bytes, downloaded_bytes, source_url);
                continue;
            }

            if let Some(sha1) = expected_sha1 {
                let hash = hex::encode(hasher.finalize());
                if hash != sha1 {
                    let _ = std::fs::remove_file(&temp_path);
                    eprintln!("SHA1 校验失败: 期望 {} 实际 {} ({})", sha1, hash, source_url);
                    continue;
                }
            }

            let _ = std::fs::remove_file(dest);
            if let Err(e) = std::fs::rename(&temp_path, dest) {
                eprintln!("重命名失败: {}", e);
                let _ = std::fs::remove_file(&temp_path);
                continue;
            }
            return Ok(());
        }
        eprintln!("源 {} 失败，切换下一个", source_url);
    }

    Err(format!("所有源均无法下载: {}", url))
}

// ========== 3.1 安装 Fabric Loader ==========
async fn install_fabric_loader(
    client: &reqwest::Client,
    version: &str,
    version_dir_name: &str,
    game_dir: &Path,
    versions_dir: &Path,
    window: &Window,
    install_fabric_api: bool,
    loader_version: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    let fabric_version = if loader_version.is_empty() {
        let _ = window.emit("download-progress", json!({ "stage": "获取 Fabric Loader 版本", "progress": 95 }));
        
        let maven_urls = vec![
            format!("https://bmclapi2.bangbang93.com/maven/net/fabricmc/fabric-loader/maven-metadata.xml"),
            "https://maven.fabricmc.net/net/fabricmc/fabric-loader/maven-metadata.xml".to_string(),
        ];
        
        let mut index_xml = String::new();
        for url in &maven_urls {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("下载已取消".to_string());
            }
            match client.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    if let Ok(text) = response.text().await {
                        index_xml = text;
                        break;
                    }
                }
                Ok(response) => eprintln!("Fabric 索引 HTTP 错误: {} - {}", url, response.status()),
                Err(e) => eprintln!("获取 Fabric 索引失败: {} - {}", url, e),
            }
        }
        
        if index_xml.is_empty() {
            return Err("所有镜像源均无法获取 Fabric 索引".to_string());
        }
        
        let mut latest_version = String::new();
        for line in index_xml.lines() {
            if line.trim().starts_with("<latest>") {
                latest_version = line.trim().replace("<latest>", "").replace("</latest>", "");
                break;
            }
        }
        
        if latest_version.is_empty() {
            return Err("未找到 Fabric Loader 版本".to_string());
        }

        // 旧 MC（< 1.20.5）不能用 0.17+ 加载器（intermediary 运行时已被移除，
        // 会导致 classTweaker 命名空间崩溃）。从 meta 拉取该 MC 的加载器列表，
        // 选最新的 ≤ 0.16.x。
        if !version_ge(version, "1.20.5") && version_ge(&latest_version, "0.17.0") {
            let meta_url = format!("https://meta.fabricmc.net/v2/versions/loader/{}", version);
            if let Ok(resp) = client.get(&meta_url).send().await {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let mut old_versions: Vec<String> = json
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|e| e["loader"]["version"].as_str().map(String::from))
                                .filter(|v| !version_ge(v, "0.17.0"))
                                .collect()
                        })
                        .unwrap_or_default();
                    old_versions.sort_by(|a, b| version_cmp(b, a));
                    if let Some(v) = old_versions.first() {
                        latest_version = v.clone();
                    }
                }
            }
        }

        latest_version
    } else {
        loader_version.to_string()
    };
    
    let _ = window.emit("download-progress", json!({ "stage": "获取 Fabric 元数据", "progress": 96 }));
    
    let meta_url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}/{}",
        version, fabric_version
    );
    
    let fabric_mirrors = vec![
        "https://bmclapi2.bangbang93.com/fabric-meta",
        "https://meta.fabricmc.net",
    ];
    
    let mut meta_json_opt = None;
    for mirror in &fabric_mirrors {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }
        let url = if mirror == &fabric_mirrors[0] {
            format!("{}/v2/versions/loader/{}/{}", mirror, version, fabric_version)
        } else {
            meta_url.clone()
        };
        
        eprintln!("尝试获取 Fabric 元数据: {}", url);
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                if let Ok(json) = response.json().await {
                    eprintln!("获取 Fabric 元数据成功: {}", mirror);
                    meta_json_opt = Some(json);
                    break;
                }
            }
            Ok(response) => eprintln!("Fabric 元数据 HTTP 错误: {} - {}", mirror, response.status()),
            Err(e) => eprintln!("获取 Fabric 元数据失败: {} - {}", mirror, e),
        }
    }
    
    let meta_json: serde_json::Value = meta_json_opt.ok_or("所有镜像源均无法获取 Fabric 元数据")?;
    
    let _ = window.emit("download-progress", json!({ "stage": "下载 Fabric 依赖库", "progress": 96 }));
    
    let launcher_meta = meta_json["launcherMeta"].as_object().ok_or("缺少 launcherMeta")?;
    
    // 关键：fabric-loader 本体不在 launcherMeta.libraries 里，它通过 loader.maven 坐标单独下载。
    // 必须先确保 fabric-loader jar 下载完成，否则启动时找不到 KnotClient 主类。
    let loader_maven = meta_json["loader"]["maven"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if !loader_maven.is_empty() {
        // 解析 net.fabricmc:fabric-loader:0.19.3
        let parts: Vec<&str> = loader_maven.split(':').collect();
        if parts.len() == 3 {
            let group_path = parts[0].replace('.', "/");
            let artifact = parts[1];
            let loader_ver = parts[2];
            let loader_path_str = format!("{}/{}/{}/{}-{}.jar", group_path, artifact, loader_ver, artifact, loader_ver);
            let loader_path = game_dir.join("libraries").join(&loader_path_str);
            if !loader_path.exists() {
                eprintln!("下载 fabric-loader 本体: {}", loader_path_str);
                let loader_url = format!("https://maven.fabricmc.net/{}", loader_path_str);
                let fabric_mirrors = vec!["https://bmclapi2.bangbang93.com"];
                if let Err(e) = download_with_mirrors(&client, &loader_url, &loader_path, None, &fabric_mirrors, &window, &cancel_flag, None).await {
                    eprintln!("fabric-loader 本体下载失败: {} - {}", loader_url, e);
                    return Err(format!("下载 fabric-loader 失败: {}", e));
                }
            } else {
                eprintln!("fabric-loader 本体已存在: {}", loader_path.display());
            }
        }
    }
    
    let lib_list: Vec<serde_json::Value> = if let Some(libs_obj) = launcher_meta["libraries"].as_object() {
        let mut all_libs = Vec::new();
        if let Some(client_libs) = libs_obj.get("client").and_then(|v| v.as_array()) {
            all_libs.extend(client_libs.iter().cloned());
        }
        if let Some(common_libs) = libs_obj.get("common").and_then(|v| v.as_array()) {
            all_libs.extend(common_libs.iter().cloned());
        }
        all_libs
    } else if let Some(libs_arr) = launcher_meta["libraries"].as_array() {
        libs_arr.clone()
    } else {
        Vec::new()
    };
    
    if !lib_list.is_empty() {
        let total_libs = lib_list.len();
        let lib_done = Arc::new(AtomicUsize::new(0));
        let lib_sem = Arc::new(Semaphore::new(16));
        let mut lib_tasks = JoinSet::new();
        // 收集失败的库，下载完成后统一重试
        let lib_failures: Arc<Mutex<Vec<(String, PathBuf)>>> = Arc::new(Mutex::new(Vec::new()));
        
        let client = Arc::new(client.clone());
        let game_dir = Arc::new(game_dir.to_path_buf());
        let window = Arc::new(window.clone());
        let cancel_flag = cancel_flag.clone();
        
        for lib in &lib_list {
            if let Some(name) = lib["name"].as_str() {
                let parts: Vec<&str> = name.split(':').collect();
                if parts.len() != 3 {
                    lib_done.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                
                let group = parts[0].replace('.', "/");
                let artifact = parts[1].to_string();
                let ver = parts[2].to_string();
                
                let url = lib["url"].as_str()
                    .unwrap_or("https://maven.fabricmc.net/")
                    .to_string();
                
                let path = format!("{}/{}/{}/{}-{}.jar", group, artifact, ver, artifact, ver);
                let base_url = url.trim_end_matches('/');
                let download_url = format!("{}/{}", base_url, path);
                let lib_path = game_dir.join("libraries").join(&path);
                
                if lib_path.exists() {
                    lib_done.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                
                let fabric_mirrors = vec![
                    "https://bmclapi2.bangbang93.com",
                ];
                
                let client_clone = client.clone();
                let window_clone = window.clone();
                let sem = lib_sem.clone();
                let done = lib_done.clone();
                let cancel_flag_clone = cancel_flag.clone();
                let failures = lib_failures.clone();
                let fabric_mirrors: Vec<String> = fabric_mirrors.iter().map(|s| s.to_string()).collect();
                
                lib_tasks.spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    if cancel_flag_clone.load(Ordering::SeqCst) {
                        done.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    let mirrors_ref: Vec<&str> = fabric_mirrors.iter().map(|s| s.as_str()).collect();
                    if let Err(e) = download_with_mirrors(&client_clone, &download_url, &lib_path, None, &mirrors_ref, &window_clone, &cancel_flag_clone, None).await {
                        eprintln!("Fabric 库下载失败: {} - {}", download_url, e);
                        failures.lock().await.push((download_url, lib_path.to_path_buf()));
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                });
            } else {
                lib_done.fetch_add(1, Ordering::Relaxed);
            }
        }
        
        let total_libs_f = total_libs;
        let lib_done_f = lib_done.clone();
        let window_clone = window.clone();
        let cancel_flag_clone_fab = cancel_flag.clone();
        tokio::spawn(async move {
            let mut last_done = 0;
            while lib_done_f.load(Ordering::Relaxed) < total_libs_f && !cancel_flag_clone_fab.load(Ordering::SeqCst) {
                let done = lib_done_f.load(Ordering::Relaxed);
                if done != last_done {
                    let _ = window_clone.emit("download-progress", json!({
                        "stage": format!("下载 Fabric 依赖库 ({}/{})", done, total_libs_f),
                        "progress": 96
                    }));
                    last_done = done;
                }
                sleep(Duration::from_millis(500)).await;
            }
        });
        
        while lib_done.load(Ordering::Relaxed) < total_libs {
            if cancel_flag.load(Ordering::SeqCst) {
                eprintln!("下载已取消，终止等待 Fabric 依赖库下载");
                return Err("下载已取消".to_string());
            }
            sleep(Duration::from_millis(100)).await;
        }
        while let Some(_) = lib_tasks.join_next().await {}
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }

        // 重试失败的 Fabric 库
        let fabric_failures = {
            let f = lib_failures.lock().await;
            f.clone()
        };
        if !fabric_failures.is_empty() {
            eprintln!("有 {} 个 Fabric 库下载失败，开始重试...", fabric_failures.len());
            for (url, lib_path) in &fabric_failures {
                if cancel_flag.load(Ordering::SeqCst) {
                    return Err("下载已取消".to_string());
                }
                let fabric_mirrors = vec!["https://bmclapi2.bangbang93.com"];
                if let Err(e) = download_with_mirrors(&client, url, lib_path, None, &fabric_mirrors, &window, &cancel_flag, None).await {
                    eprintln!("Fabric 库重试仍失败: {} - {}", url, e);
                }
            }
        }
    }
    
    let _ = window.emit("download-progress", json!({ "stage": "生成 Fabric 版本 JSON", "progress": 97 }));
    
    modify_version_json_for_fabric(versions_dir, version_dir_name, &fabric_version, &meta_json, &loader_maven)?;
    
    if install_fabric_api {
        let _ = window.emit("download-progress", json!({ "stage": "下载 Fabric API", "progress": 98 }));
        install_fabric_api_mod(client, version, window, versions_dir, cancel_flag).await?;
    }
    
    Ok(())
}

// ========== 辅助：把安装器生成的独立版本目录注入合并回原版 ==========
// Forge/NeoForge 官方安装器会在 versions/ 下生成独立目录（如 26.2-forge-47.x/）。
// 为保持"一个版本"的用户体验，把它的 json 内容合并进原版 json 后删除该目录。
async fn inject_installer_version_into_original(
    game_dir: &Path,
    version: &str,
    version_dir_name: &str,
    versions_dir: &Path,
    before_dirs: &std::collections::HashSet<String>,
    loader_name: &str,
) -> Result<(), String> {
    let versions_root = game_dir.join("versions");

    // 找出安装器生成的新目录
    let mut generated_dir: Option<String> = None;
    if let Ok(rd) = fs::read_dir(&versions_root) {
        for entry in rd.filter_map(|e| e.ok()) {
            if !entry.path().is_dir() {
                continue;
            }
            if let Ok(name) = entry.file_name().into_string() {
                if name != version && name != version_dir_name && !before_dirs.contains(&name) {
                    generated_dir = Some(name);
                    break;
                }
            }
        }
    }

    let generated_dir = generated_dir.ok_or_else(|| {
        format!(
            "{} 安装器执行成功但未生成版本目录，可能已存在旧版本或安装器行为异常",
            loader_name
        )
    })?;

    let generated_json_path = versions_root
        .join(&generated_dir)
        .join(format!("{}.json", generated_dir));
    if !generated_json_path.exists() {
        let _ = fs::remove_dir_all(versions_root.join(&generated_dir));
        return Err(format!("{} 安装器生成的版本目录缺少 json: {}", loader_name, generated_dir));
    }

    // 读取安装器生成的 json
    let generated_json_str = fs::read_to_string(&generated_json_path)
        .map_err(|e| format!("读取 {} 生成 json 失败: {}", loader_name, e))?;
    let generated_json: serde_json::Value = serde_json::from_str(&generated_json_str)
        .map_err(|e| format!("解析 {} 生成 json 失败: {}", loader_name, e))?;

    // 读取原版 json
    let original_json_path = versions_dir.join(format!("{}.json", version_dir_name));
    let original_json_str = fs::read_to_string(&original_json_path)
        .map_err(|e| format!("读取原版 json 失败: {}", e))?;
    let mut merged: serde_json::Value = serde_json::from_str(&original_json_str)
        .map_err(|e| format!("解析原版 json 失败: {}", e))?;

    // 注入 mainClass
    if let Some(main_class) = generated_json["mainClass"].as_str() {
        merged["mainClass"] = serde_json::json!(main_class);
        eprintln!("注入 {} mainClass: {}", loader_name, main_class);
    }

    // 合并 libraries：与 PCL2 MergeJson 一致 —— 原版在前、加载器在后直接拼接。
    // 官方启动器与 PCL2 都按 classpath 顺序取第一个同名库，拼接即为等价行为。
    if let Some(gen_libs) = generated_json["libraries"].as_array() {
        let mut orig_libs = merged["libraries"].as_array().cloned().unwrap_or_default();
        orig_libs.extend(gen_libs.iter().cloned());
        merged["libraries"] = serde_json::json!(orig_libs);
    }

    // 合并 arguments（game/jvm 直接拼接，PCL2 行为）。
    // 注意：绝不能按字符串去重——--add-opens/--add-exports 等旗标会合法重复，
    // 去重会删掉旗标导致参数对错位（JVM 崩溃 "找不到主类"）。
    if let Some(gen_args) = generated_json["arguments"].as_object() {
        let mut orig_args = merged["arguments"].as_object().cloned().unwrap_or_default();
        for key in ["game", "jvm"] {
            if let Some(gen_list) = gen_args.get(key).and_then(|v| v.as_array()) {
                let mut orig_list = orig_args
                    .get(key)
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                orig_list.extend(gen_list.iter().cloned());
                orig_args.insert(key.to_string(), serde_json::json!(orig_list));
            }
        }
        merged["arguments"] = serde_json::json!(orig_args);
    }

    // 旧版 Forge（1.7~1.12）使用 minecraftArguments 字符串：拼接原版与加载器参数（PCL2 行为）
    if let Some(gen_mc_args) = generated_json["minecraftArguments"].as_str() {
        let gen_trimmed = gen_mc_args.trim();
        if !gen_trimmed.is_empty() {
            let cur = merged["minecraftArguments"].as_str().unwrap_or("").trim();
            let combined = if cur.is_empty() {
                gen_trimmed.to_string()
            } else {
                format!("{} {}", cur, gen_trimmed)
            };
            merged["minecraftArguments"] = serde_json::json!(combined);
        }
    }

    // 去掉 inheritsFrom（注入后不再依赖独立目录）
    if merged.get("inheritsFrom").is_some() {
        merged.as_object_mut().unwrap().remove("inheritsFrom");
    }
    // 清理安装器 json 中的注释键（PCL2 MergeJson 同样移除）
    if let Some(obj) = merged.as_object_mut() {
        obj.remove("_comment_");
    }
    // 确保 id 保持原版名
    merged["id"] = serde_json::json!(version_dir_name);

    // 写回原版 json
    fs::write(
        &original_json_path,
        serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    // 删除安装器生成的独立目录
    let _ = fs::remove_dir_all(versions_root.join(&generated_dir));
    eprintln!("{} 注入完成，已删除独立目录: {}", loader_name, generated_dir);

    Ok(())
}

// ========== 3.1.1 运行 Java 安装器（Forge/NeoForge 共用：超时保护 + 输出记录） ==========
fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// 安装器运行前，解析安装器内的 version.json，用启动器自己的健壮下载客户端
/// 预下载全部加载器依赖库。Java 安装器自带下载在部分网络环境下失败率高
/// （表现为 "These libraries failed to download"）；预下载后安装器会校验跳过。
async fn predownload_installer_libs(
    client: &reqwest::Client,
    installer_path: &Path,
    game_dir: &Path,
    window: &Window,
    cancel_flag: &Arc<AtomicBool>,
    loader_name: &str,
) -> Result<(), String> {
    use std::io::Read as _;

    if cancel_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    let file = fs::File::open(installer_path).map_err(|e| format!("打开安装器失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析安装器失败: {}", e))?;
    let mut json_text = String::new();
    archive
        .by_name("version.json")
        .map_err(|_| "安装器缺少 version.json".to_string())?
        .read_to_string(&mut json_text)
        .map_err(|e| format!("读取 version.json 失败: {}", e))?;
    let j: serde_json::Value = serde_json::from_str(&json_text)
        .map_err(|e| format!("解析 version.json 失败: {}", e))?;

    let libs = j["libraries"].as_array().cloned().unwrap_or_default();
    let total = libs.len();
    let mut done = 0usize;
    for lib in &libs {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }
        let artifact = &lib["downloads"]["artifact"];
        let path = match artifact["path"].as_str() {
            Some(p) => p,
            None => continue, // 无 artifact（如只有 name 的依赖）交给安装器处理
        };
        let sha1 = artifact["sha1"].as_str().map(String::from);
        let url = artifact["url"].as_str().unwrap_or("");

        let dest = game_dir.join("libraries").join(path);
        if dest.exists() {
            if let Ok(content) = fs::read(&dest) {
                let valid = match &sha1 {
                    Some(h) => sha1_hex(&content).eq_ignore_ascii_case(h),
                    None => true,
                };
                if valid {
                    done += 1;
                    continue;
                }
            }
        }
        if url.is_empty() {
            continue;
        }
        download_mod_file(client, url, &dest, sha1.as_deref(), None, None)
            .await
            .map_err(|e| format!("预下载 {} 依赖库 {} 失败: {}", loader_name, path, e))?;
        done += 1;
        if total > 0 {
            let _ = window.emit("download-progress", json!({
                "stage": format!("下载 {} 依赖库 ({}/{})", loader_name, done, total),
                "progress": 96
            }));
        }
    }
    dl_log(&format!("{} 依赖库预下载完成 ({}/{})", loader_name, done, total));
    Ok(())
}

async fn run_installer_java(
    java_path: &str,
    installer_path: &Path,
    game_dir: &Path,
    loader_name: &str,
    window: &Window,
) -> Result<(), String> {
    let java = java_path.to_string();
    let jar = installer_path.to_path_buf();
    let dir = game_dir.to_path_buf();
    let name = loader_name.to_string();

    dl_log(&format!("运行 {} 安装器: {} -jar {} --installClient {}", name, java, jar.display(), dir.display()));

    // 安装器运行期间的心跳更新：每 5 秒更新一次阶段文字，进度保持在 97
    let running = Arc::new(AtomicBool::new(true));
    let running_flag = running.clone();
    let heartbeat_window = window.clone();
    let heartbeat_name = name.clone();
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        while running_flag.load(Ordering::Relaxed) {
            sleep(Duration::from_secs(5)).await;
            if !running_flag.load(Ordering::Relaxed) {
                break;
            }
            let elapsed = start.elapsed().as_secs();
            let _ = heartbeat_window.emit("download-progress", json!({
                "stage": format!("正在运行 {} 安装器（已用 {} 秒，下载依赖库中，请耐心等待）", heartbeat_name, elapsed),
                "progress": 97
            }));
        }
    });

    let result = tokio::time::timeout(
        Duration::from_secs(15 * 60),
        tokio::task::spawn_blocking(move || {
            // 显式传入 --installClient <游戏目录>：新版统一安装器（Forge 65+/NeoForge 26.x）
            // 不带目录时会默认写入 macOS 官方目录 ~/Library/Application Support/minecraft，
            // 绝不能碰用户的官方启动器目录。
            #[cfg(windows)]
            let mut install_cmd = {
                use std::os::windows::process::CommandExt;
                let mut c = std::process::Command::new(&java);
                c.creation_flags(0x08000000); // CREATE_NO_WINDOW：安装器不弹黑窗口
                c
            };
            #[cfg(not(windows))]
            let mut install_cmd = std::process::Command::new(&java);

            let output = install_cmd
                .arg("-jar")
                .arg(&jar)
                .arg("--installClient")
                .arg(&dir)
                .current_dir(&dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| format!("启动 {} 安装器失败: {}", name, e))?;

            let out_text = String::from_utf8_lossy(&output.stdout).to_string();
            let err_text = String::from_utf8_lossy(&output.stderr).to_string();

            if !output.status.success() {
                dl_log(&format!(
                    "{} 安装器失败，退出码 {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
                    name,
                    output.status.code().unwrap_or(-1),
                    tail_lines(&out_text, 40),
                    tail_lines(&err_text, 40),
                ));
                return Err(format!(
                    "{} 安装器执行失败，退出码: {}\n{}\n{}",
                    name,
                    output.status.code().unwrap_or(-1),
                    tail_lines(&out_text, 15),
                    tail_lines(&err_text, 15),
                ));
            }

            dl_log(&format!(
                "{} 安装器执行成功\n--- 输出末尾 ---\n{}",
                name,
                tail_lines(&out_text, 12)
            ));
            Ok(())
        }),
    )
    .await;

    running.store(false, Ordering::Relaxed);

    match result {
        Err(_) => Err(format!("{} 安装器超时（15 分钟），已放弃等待", loader_name)),
        Ok(Err(e)) => Err(format!("{} 安装任务失败: {}", loader_name, e)),
        Ok(Ok(Err(e))) => Err(e),
        Ok(Ok(Ok(()))) => Ok(()),
    }
}

// ========== 3.2 安装 Forge Loader ==========
async fn install_forge_loader(
    client: &reqwest::Client,
    version: &str,
    version_dir_name: &str,
    game_dir: &Path,
    versions_dir: &Path,
    window: &Window,
    loader_version: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    let forge_version = if loader_version.is_empty() {
        let _ = window.emit("download-progress", json!({ "stage": "获取 Forge 版本", "progress": 95 }));
        
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }

        let mc_version = version.replace("snapshot-", "");
        
        let forge_maven_urls = vec![
            format!("https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/maven-metadata.xml"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml".to_string(),
            "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml".to_string(),
        ];
        
        let mut index_xml = String::new();
        for url in &forge_maven_urls {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("下载已取消".to_string());
            }
            match client.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    if let Ok(text) = response.text().await {
                        index_xml = text;
                        break;
                    }
                }
                Ok(response) => eprintln!("Forge 索引 HTTP 错误: {} - {}", url, response.status()),
                Err(e) => eprintln!("获取 Forge 索引失败: {} - {}", url, e),
            }
        }
        
        if index_xml.is_empty() {
            return Err("所有镜像源均无法获取 Forge 索引".to_string());
        }
        
        let mut found_version = String::new();
        for line in index_xml.lines() {
            if line.contains(&mc_version) {
                let start = line.find('>').unwrap_or(0) + 1;
                let end = line.rfind('<').unwrap_or(line.len());
                found_version = line[start..end].to_string();
                break;
            }
        }
        
        if found_version.is_empty() {
            return Err(format!("未找到 Minecraft {} 的 Forge 版本", mc_version));
        }
        
        found_version
    } else {
        loader_version.to_string()
    };
    
    let _ = window.emit("download-progress", json!({ "stage": format!("下载 Forge {}", forge_version), "progress": 96 }));
    
    let forge_url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{}/forge-{}-installer.jar",
        forge_version, forge_version
    );
    
    let installer_path = game_dir.join(format!("forge-{}-installer.jar", forge_version));
    
    let forge_mirrors = vec![
        "https://bmclapi2.bangbang93.com",
    ];
    
    download_with_mirrors(client, &forge_url, &installer_path, None, &forge_mirrors, window, cancel_flag, None)
        .await
        .map_err(|e| format!("下载 Forge 安装器失败: {}", e))?;
    
    if cancel_flag.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&installer_path);
        return Err("下载已取消".to_string());
    }

    let _ = window.emit("download-progress", json!({ "stage": "运行 Forge 安装器", "progress": 97 }));
    
    // 预下载 Forge 依赖库（安装器自带下载在部分网络下失败率高）
    predownload_installer_libs(client, &installer_path, game_dir, window, cancel_flag, "Forge").await?;
    
    let java_path = get_java_path_from_env();

    // Forge 安装器要求游戏目录存在 launcher_profiles.json（官方启动器会生成），
    // 缺失时创建一个最小 profile，否则安装器直接报 "you need to run the launcher first"
    let profiles_path = game_dir.join("launcher_profiles.json");
    if !profiles_path.exists() {
        let minimal = serde_json::json!({
            "profiles": {},
            "settings": {},
            "version": 3
        });
        fs::write(&profiles_path, serde_json::to_string(&minimal).unwrap_or_default())
            .map_err(|e| format!("创建 launcher_profiles.json 失败: {}", e))?;
        dl_log("已创建最小 launcher_profiles.json");
    }
    
    // 记录安装前 versions 目录下的已有目录，安装后对比找出安装器生成的目录
    let versions_root = game_dir.join("versions");
    let before_dirs: std::collections::HashSet<String> = match fs::read_dir(&versions_root) {
        Ok(rd) => rd.filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => std::collections::HashSet::new(),
    };
    
    run_installer_java(&java_path, &installer_path, game_dir, "Forge", window).await?;
    let _ = fs::remove_file(&installer_path);

    // 安装器已生成独立版本目录，现在把它注入合并回原版 json（保持单版本目录）
    inject_installer_version_into_original(game_dir, version, version_dir_name, versions_dir, &before_dirs, "Forge").await?;
    
    Ok(())
}

// ========== 3.3 安装 NeoForge Loader ==========
async fn install_neoforge_loader(
    client: &reqwest::Client,
    version: &str,
    version_dir_name: &str,
    game_dir: &Path,
    versions_dir: &Path,
    _mirrors: &Vec<&str>,
    window: &Window,
    loader_version: &str,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("下载已取消".to_string());
    }

    let neoforge_version = if loader_version.is_empty() {
        let _ = window.emit("download-progress", json!({ "stage": "获取 NeoForge 版本", "progress": 95 }));
        
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("下载已取消".to_string());
        }

        let mc_version = version.replace("snapshot-", "");
        
        let neoforge_maven_urls = vec![
            format!("https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/maven-metadata.xml"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml".to_string(),
        ];
        
        let mut index_xml = String::new();
        for url in &neoforge_maven_urls {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("下载已取消".to_string());
            }
            match client.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    if let Ok(text) = response.text().await {
                        index_xml = text;
                        break;
                    }
                }
                Ok(response) => eprintln!("NeoForge 索引 HTTP 错误: {} - {}", url, response.status()),
                Err(e) => eprintln!("获取 NeoForge 索引失败: {} - {}", url, e),
            }
        }
        
        if index_xml.is_empty() {
            return Err("所有镜像源均无法获取 NeoForge 索引".to_string());
        }
        
        // 遍历全部版本，取匹配 MC 版本的最新一个（maven-metadata 按升序排列）
        let mut found_version = String::new();
        for line in index_xml.lines() {
            if !line.contains("<version>") {
                continue;
            }
            let start = line.find('>').unwrap_or(0) + 1;
            let end = line.rfind('<').unwrap_or(line.len());
            let ver = line[start..end].trim();
            if neoforge_version_matches_mc(ver, &mc_version)
                && (found_version.is_empty()
                    || version_cmp(ver, &found_version) == std::cmp::Ordering::Greater)
            {
                found_version = ver.to_string();
            }
        }
        
        if found_version.is_empty() {
            return Err(format!("未找到 Minecraft {} 的 NeoForge 版本", mc_version));
        }
        
        found_version
    } else {
        loader_version.to_string()
    };
    
    let _ = window.emit("download-progress", json!({ "stage": format!("下载 NeoForge {}", neoforge_version), "progress": 96 }));
    
    let installer_path = game_dir.join(format!("neoforge-{}-installer.jar", neoforge_version));
    
    let neoforge_mirrors = vec![
        "https://bmclapi2.bangbang93.com",
    ];
    
    // NeoForge 安装器位于 maven 仓库 releases/snapshots 路径下（beta 版多数也在 releases），
    // 依次尝试两个仓库，全部失败才报错。
    let mut last_err: Option<String> = None;
    for repo in ["releases", "snapshots"] {
        let neoforge_url = format!(
            "https://maven.neoforged.net/{}/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
            repo, neoforge_version, neoforge_version
        );
        dl_log(&format!("尝试下载 NeoForge 安装器: {}", neoforge_url));
        match download_with_mirrors(client, &neoforge_url, &installer_path, None, &neoforge_mirrors, window, cancel_flag, None).await {
            Ok(()) => { last_err = None; break; }
            Err(e) => { last_err = Some(format!("{} ({})", e, repo)); }
        }
    }
    if let Some(e) = last_err {
        return Err(format!("下载 NeoForge 安装器失败: {}", e));
    }
    
    if cancel_flag.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&installer_path);
        return Err("下载已取消".to_string());
    }

    let _ = window.emit("download-progress", json!({ "stage": "运行 NeoForge 安装器", "progress": 97 }));
    
    // 预下载 NeoForge 依赖库（安装器自带下载在部分网络下失败率高）
    predownload_installer_libs(client, &installer_path, game_dir, window, cancel_flag, "NeoForge").await?;
    
    let java_path = get_java_path_from_env();

    // NeoForge 安装器同样要求 launcher_profiles.json 存在
    let profiles_path = game_dir.join("launcher_profiles.json");
    if !profiles_path.exists() {
        let minimal = serde_json::json!({
            "profiles": {},
            "settings": {},
            "version": 3
        });
        fs::write(&profiles_path, serde_json::to_string(&minimal).unwrap_or_default())
            .map_err(|e| format!("创建 launcher_profiles.json 失败: {}", e))?;
        dl_log("已创建最小 launcher_profiles.json");
    }
    
    // 记录安装前 versions 目录下的已有目录，安装后对比找出安装器生成的目录
    let versions_root = game_dir.join("versions");
    let before_dirs: std::collections::HashSet<String> = match fs::read_dir(&versions_root) {
        Ok(rd) => rd.filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => std::collections::HashSet::new(),
    };
    
    run_installer_java(&java_path, &installer_path, game_dir, "NeoForge", window).await?;
    let _ = fs::remove_file(&installer_path);

    // 安装器已生成独立版本目录，注入合并回原版 json（保持单版本目录）
    inject_installer_version_into_original(game_dir, version, version_dir_name, versions_dir, &before_dirs, "NeoForge").await?;
    
    Ok(())
}

// ========== 3.4 修改版本 JSON 以支持 Fabric ==========
fn modify_version_json_for_fabric(versions_dir: &Path, version: &str, _loader_version: &str, meta_json: &serde_json::Value, loader_maven: &str) -> Result<(), String> {
    let json_path = versions_dir.join(format!("{}.json", version));
    let content = fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
    
    let mut json: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    
    let launcher_meta = meta_json["launcherMeta"].as_object().ok_or("缺少 launcherMeta")?;
    
    eprintln!("launcherMeta keys: {:?}", launcher_meta.keys().collect::<Vec<_>>());
    
    // ========== mainClass ==========
    // 先尝试 v1 格式（对象 {client, server}），再回退 v0 格式（字符串）
    if let Some(main_class_obj) = launcher_meta["mainClass"].as_object() {
        if let Some(client_main) = main_class_obj.get("client").and_then(|v| v.as_str()) {
            json["mainClass"] = serde_json::json!(client_main);
            eprintln!("已设置 mainClass (v1 client): {}", client_main);
        } else if let Some(server_main) = main_class_obj.get("server").and_then(|v| v.as_str()) {
            json["mainClass"] = serde_json::json!(server_main);
            eprintln!("已设置 mainClass (v1 server): {}", server_main);
        }
    } else if let Some(main_class) = launcher_meta["mainClass"].as_str() {
        json["mainClass"] = serde_json::json!(main_class);
        eprintln!("已设置 mainClass (v0): {}", main_class);
    } else {
        eprintln!("警告: 未找到 mainClass");
    }
    
    // ========== 把 fabric-loader 本体加入 libraries ==========
    // fabric-loader 本体不在 launcherMeta.libraries 里，必须单独加入，否则启动时找不到 KnotClient
    if !loader_maven.is_empty() {
        if let Some(libraries) = json["libraries"].as_array_mut() {
            // 检查是否已存在
            let exists = libraries.iter().any(|l| {
                l["name"].as_str().map(|n| n == loader_maven).unwrap_or(false)
            });
            if !exists {
                // 标准 Fabric loader 库条目：name + url
                let loader_lib = serde_json::json!({
                    "name": loader_maven,
                    "url": "https://maven.fabricmc.net/"
                });
                libraries.push(loader_lib);
                eprintln!("已将 fabric-loader 加入 libraries: {}", loader_maven);
            }
        }
    }
    
    // ========== libraries ==========
    // 先尝试 v1 格式（对象 {client, common, server}），再回退 v0 格式（数组）
    if let Some(libraries_obj) = launcher_meta["libraries"].as_object() {
        if let Some(libraries) = json["libraries"].as_array_mut() {
            let mut count = 0;
            if let Some(client_libs) = libraries_obj.get("client").and_then(|v| v.as_array()) {
                for lib in client_libs {
                    libraries.push(lib.clone());
                    count += 1;
                }
            }
            if let Some(common_libs) = libraries_obj.get("common").and_then(|v| v.as_array()) {
                for lib in common_libs {
                    libraries.push(lib.clone());
                    count += 1;
                }
            }
            eprintln!("已添加 {} 个库 (v1 格式)", count);
        }
    } else if let Some(meta_libraries) = launcher_meta["libraries"].as_array() {
        if let Some(libraries) = json["libraries"].as_array_mut() {
            for lib in meta_libraries {
                libraries.push(lib.clone());
            }
            eprintln!("已添加 {} 个库 (v0 格式)", meta_libraries.len());
        }
    } else {
        eprintln!("警告: 未找到 libraries");
    }
    
    // ========== arguments ==========
    // 先尝试 v1 格式（{client: {game, jvm}, server: {game, jvm}}），再回退 v0 格式（{game, jvm}）
    if let Some(meta_args) = launcher_meta.get("arguments").and_then(|v| v.as_object()) {
        if let Some(args) = json["arguments"].as_object_mut() {
            // 检查是否是 v1 格式（有 client 键）
            if let Some(client_args) = meta_args.get("client").and_then(|v| v.as_object()) {
                eprintln!("arguments 为 v1 格式，使用 client 参数");
                merge_args(client_args, args);
            } else {
                // v0 格式
                eprintln!("arguments 为 v0 格式");
                merge_args(meta_args, args);
            }
        }
    }
    
    let new_content = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    fs::write(&json_path, new_content).map_err(|e| e.to_string())?;
    
    eprintln!("版本 JSON 已保存: {}", json_path.display());
    Ok(())
}

fn merge_args(source: &serde_json::Map<String, serde_json::Value>, target: &mut serde_json::Map<String, serde_json::Value>) {
    if let Some(game_args) = source.get("game").and_then(|v| v.as_array()) {
        if let Some(existing_game_args) = target["game"].as_array_mut() {
            for arg in game_args {
                existing_game_args.push(arg.clone());
            }
        } else {
            target["game"] = serde_json::json!(game_args);
        }
    }
    if let Some(jvm_args) = source.get("jvm").and_then(|v| v.as_array()) {
        if let Some(existing_jvm_args) = target["jvm"].as_array_mut() {
            for arg in jvm_args {
                existing_jvm_args.push(arg.clone());
            }
        } else {
            target["jvm"] = serde_json::json!(jvm_args);
        }
    }
}

// ========== 3.5 安装 Fabric API 模组 ==========
async fn install_fabric_api_mod(
    client: &reqwest::Client,
    version: &str,
    window: &Window,
    versions_dir: &Path,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), String> {
    // 直接使用完整 MC 版本号查询（如 1.20.1、26.2、26.2-snapshot-8）。
    // 快照 id 本身也带 "snapshot-"（如 26.2-snapshot-8），必须原样传给 Modrinth，
    // 不能 replace 掉，否则会变成不存在的 26.2-8 导致匹配不到 Fabric API。
    let mc_version = version;
    
    let _ = window.emit("download-progress", json!({ "stage": "从 Modrinth 获取 Fabric API", "progress": 98 }));
    
    let modrinth_api_url = format!(
        "https://api.modrinth.com/v2/project/P7dR8mSH/version?game_versions=[\"{}\"]&loaders=[\"fabric\"]",
        mc_version
    );
    
    let response = client.get(&modrinth_api_url).send().await
        .map_err(|e| format!("获取 Fabric API 版本失败: {}", e))?;
    
    let versions: serde_json::Value = response.json().await
        .map_err(|e| format!("解析 Fabric API 版本失败: {}", e))?;
    
    let versions_array = versions.as_array().ok_or("无效的版本列表格式")?;
    
    if versions_array.is_empty() {
        return Err(format!("未找到适用于 Minecraft {} 的 Fabric API 版本", mc_version));
    }
    
    let latest_version = &versions_array[0];
    let version_id = latest_version["version_number"].as_str().ok_or("缺少版本号")?;
    let files = latest_version["files"].as_array().ok_or("缺少文件列表")?;
    
    let primary_file = files.iter()
        .find(|f| f["primary"].as_bool().unwrap_or(false))
        .or_else(|| files.first())
        .ok_or("未找到下载文件")?;
    
    let download_url = primary_file["url"].as_str().ok_or("缺少下载 URL")?;
    let filename = primary_file["filename"].as_str().ok_or("缺少文件名")?;
    // Modrinth 提供 sha1 校验值，用它防止 CDN 截断的坏文件被保存
    let file_sha1 = primary_file["hashes"]["sha1"]
        .as_str()
        .unwrap_or("")
        .to_string();
    
    let _ = window.emit("download-progress", json!({ 
        "stage": format!("下载 Fabric API {}", version_id), 
        "progress": 98 
    }));
    
    let mods_dir = versions_dir.join("mods");
    fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    
    let api_path = mods_dir.join(filename);
    let expected_sha1: Option<&str> = if file_sha1.is_empty() { None } else { Some(&file_sha1) };
    let fabric_mirrors = vec!["https://bmclapi2.bangbang93.com"];
    download_with_mirrors(client, download_url, &api_path, expected_sha1, &fabric_mirrors, window, cancel_flag, None)
        .await
        .map_err(|e| format!("下载 Fabric API 失败: {}", e))?;
    
    Ok(())
}

// ========== 3.6 获取 Fabric Loader 版本列表 ==========
#[tauri::command]
async fn get_fabric_versions(version: String) -> Result<Vec<String>, String> {
    if !version_ge(&version, "1.14") {
        return Ok(Vec::new());
    }
    
    let client = reqwest::Client::builder()
        .user_agent("Lumia-Launcher/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{}", version);
    let mut json_opt: Option<serde_json::Value> = None;

    // 官方 + BMCLAPI fabric-meta 镜像回退
    let mut urls = vec![url.clone()];
    urls.push(format!("https://bmclapi2.bangbang93.com/fabric-meta/v2/versions/loader/{}", version));
    urls.push(format!("https://bmclapi.bangbang93.com/fabric-meta/v2/versions/loader/{}", version));

    for u in &urls {
        match client.get(u).send().await {
            Ok(response) if response.status().is_success() => {
                if let Ok(json) = response.json().await {
                    json_opt = Some(json);
                    break;
                }
            }
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    let json = json_opt.ok_or("获取 Fabric 版本失败")?;
    
    let mut versions = Vec::new();
    
    if let Some(loaders) = json.as_array() {
        for loader in loaders {
            if let Some(loader_info) = loader["loader"].as_object() {
                if let Some(loader_ver) = loader_info["version"].as_str() {
                    versions.push(loader_ver.to_string());
                }
            }
        }
    }
    
    versions.sort_by(|a, b| version_cmp(b, a));

    // 关键：MC < 1.20.5 只能用 fabric-loader ≤ 0.16.x。
    // 0.17+ 的加载器不再为旧 MC 提供 intermediary 运行时，会直接崩溃：
    // "ClassTweakerFormatException: Namespace (intermediary) does not match
    //  current runtime namespace (official)"
    if !version_ge(&version, "1.20.5") {
        versions.retain(|v| !version_ge(v, "0.17.0"));
    }

    Ok(versions)
}

// ========== 3.7 获取 Forge 版本列表 ==========
#[tauri::command]
async fn get_forge_versions(version: String) -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Lumia-Launcher/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let mut xml_opt: Option<String> = None;
    let urls = [
        "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
        "https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/maven-metadata.xml",
        "https://bmclapi.bangbang93.com/maven/net/minecraftforge/forge/maven-metadata.xml",
    ];

    for u in urls {
        match client.get(u).send().await {
            Ok(response) if response.status().is_success() => {
                if let Ok(text) = response.text().await {
                    xml_opt = Some(text);
                    break;
                }
            }
            _ => continue,
        }
    }
    let xml = xml_opt.ok_or("获取 Forge 版本失败")?;
    
    let mut versions = Vec::new();
    let mc_version_prefix = format!("{}-", version);
    
    for line in xml.lines() {
        if line.contains("<version>") {
            let start = line.find('>').unwrap_or(0) + 1;
            let end = line.rfind('<').unwrap_or(line.len());
            let ver = line[start..end].trim();
            if ver.starts_with(&mc_version_prefix) {
                versions.push(ver.to_string());
            }
        }
    }
    
    versions.sort_by(|a, b| version_cmp(b, a));
    Ok(versions)
}

// ========== 3.8 获取 NeoForge 版本列表 ==========
// NeoForge 版本号格式（与 Forge 的 "26.2-65.0.0" 不同）：
//   MC 1.x   → "21.1.248"（去掉前导 "1."，两段 MC 版本 + 构建号；1.21 → 21.0.x）
//   MC 26.x  → "26.2.0.57"（三段 MC 版本 + 构建号；26.2 → 26.2.0.x）
fn neoforge_version_matches_mc(ver: &str, mc: &str) -> bool {
    let base = ver.split('-').next().unwrap_or(ver);
    let segs: Vec<&str> = base.split('.').collect();
    let mut mc_segs: Vec<&str> = mc.split('.').collect();
    if mc_segs.first() == Some(&"1") {
        mc_segs.remove(0); // 去掉前导 "1."
    }
    let target_len = if mc.starts_with("1.") { 2 } else { 3 };
    while mc_segs.len() < target_len {
        mc_segs.push("0"); // 26.2 → 26.2.0；1.21 → 21.0
    }
    segs.len() == target_len + 1 && &segs[..target_len] == &mc_segs[..]
}

#[tauri::command]
async fn get_neoforge_versions(version: String) -> Result<Vec<String>, String> {
    if !version_ge(&version, "1.20.1") {
        return Ok(Vec::new());
    }
    
    let client = reqwest::Client::builder()
        .user_agent("Lumia-Launcher/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let mut xml_opt: Option<String> = None;
    let urls = [
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml",
        "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/maven-metadata.xml",
        "https://bmclapi.bangbang93.com/maven/net/neoforged/neoforge/maven-metadata.xml",
    ];

    for u in urls {
        match client.get(u).send().await {
            Ok(response) if response.status().is_success() => {
                if let Ok(text) = response.text().await {
                    xml_opt = Some(text);
                    break;
                }
            }
            _ => continue,
        }
    }
    let xml = xml_opt.ok_or("获取 NeoForge 版本失败")?;
    
    let mut versions = Vec::new();
    
    for line in xml.lines() {
        if line.contains("<version>") {
            let start = line.find('>').unwrap_or(0) + 1;
            let end = line.rfind('<').unwrap_or(line.len());
            let ver = line[start..end].trim();
            if neoforge_version_matches_mc(ver, &version) {
                versions.push(ver.to_string());
            }
        }
    }
    
    // 新版本在前；同版本号时稳定版优先于 beta
    versions.sort_by(|a, b| {
        version_cmp(b, a).then(a.contains("beta").cmp(&b.contains("beta")))
    });
    Ok(versions)
}

// ========== 3.10 模组搜索 / 安装（Modrinth / CurseForge） ==========

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModSearchResult {
    id: String,
    name: String,
    summary: String,
    icon_url: String,
    source: String,
    loaders: Vec<String>,
    game_versions: Vec<String>,
    downloads: u64,
    updated_at: String,
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn modrinth_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Lumia-Launcher/0.1 (Lumia Launcher; modrinth search)")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))
}

#[tauri::command]
async fn search_mods(
    app_handle: tauri::AppHandle,
    query: String,
    loader: String,
    mc_version: String,
    source: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<ModSearchResult>, String> {
    let client = modrinth_client()?;

    match source.as_str() {
        // CurseForge：官方 API（需 Key）
        "curseforge" => {
            search_curseforge_mods(&app_handle, &client, query, loader, mc_version, offset, limit).await
        }
        // Modrinth（默认）
        _ => {
            search_modrinth_mods(&client, query, loader, mc_version, offset, limit).await
        }
    }
}

/// 搜索 Modrinth 模组
async fn search_modrinth_mods(
    client: &reqwest::Client,
    query: String,
    loader: String,
    mc_version: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<ModSearchResult>, String> {
    // facets：外层 OR、内层 AND；过滤模组类型 + 加载器 + MC 版本
    let mut facets: Vec<Vec<String>> = vec![vec!["project_type:mod".to_string()]];
    if !loader.is_empty() {
        facets.push(vec![format!("categories:{}", loader)]);
    }
    if !mc_version.is_empty() {
        facets.push(vec![format!("versions:{}", mc_version)]);
    }

    let facets_json = serde_json::to_string(&facets).unwrap_or_default();
    let url = format!(
        "https://api.modrinth.com/v2/search?query={}&facets={}&index=downloads&offset={}&limit={}",
        url_encode(&query),
        url_encode(&facets_json),
        offset,
        limit.min(100).max(1)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Modrinth 搜索请求失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Modrinth 搜索失败: HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析搜索结果失败: {}", e))?;

    let mut results = Vec::new();
    if let Some(hits) = json["hits"].as_array() {
        for hit in hits {
            let loaders: Vec<String> = hit["categories"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .filter(|s| ["fabric", "forge", "neoforge", "quilt"].contains(&s.as_str()))
                        .collect()
                })
                .unwrap_or_default();
            let game_versions: Vec<String> = hit["versions"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            results.push(ModSearchResult {
                id: hit["project_id"].as_str().unwrap_or("").to_string(),
                name: hit["title"].as_str().unwrap_or("").to_string(),
                summary: hit["description"].as_str().unwrap_or("").to_string(),
                icon_url: hit["icon_url"].as_str().unwrap_or("").to_string(),
                source: "modrinth".to_string(),
                loaders,
                game_versions,
                downloads: hit["downloads"].as_u64().unwrap_or(0),
                updated_at: hit["date_modified"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Ok(results)
}

/// CurseForge 模组加载器类型 ID（api.curseforge.com 文档）
fn curseforge_loader_type(loader: &str) -> u32 {
    match loader {
        "forge" => 1,
        "fabric" => 4,
        "quilt" => 5,
        "neoforge" => 6,
        _ => 0,
    }
}

/// CurseForge API Key：环境变量（CURSEFORGE_API_KEY / PCL 同款）优先，其次设置里的配置
async fn curseforge_api_key(app_handle: &tauri::AppHandle) -> Option<String> {
    for env_name in ["CURSEFORGE_API_KEY", "PCL_CURSEFORGE_API_KEY"] {
        if let Ok(k) = std::env::var(env_name) {
            let k = k.trim().to_string();
            if !k.is_empty() {
                return Some(k);
            }
        }
    }
    let state = app_handle.state::<AppState>();
    let config = state.config.lock().await;
    if let Some(k) = config.curseforge_api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        return Some(k.clone());
    }
    // 内置默认 Key：用户自己的 CurseForge API Key。
    Some("$2a$10$JRlwNRdcHAr5As6i0MJ1Xu2JWFlNR5aUORM8SByZNXSXT4kiVPN3e".to_string())
}

/// 发 CurseForge API 请求：仅官方 api.curseforge.com（需要 Key；不使用第三方镜像）。
/// body 为 Some 时发 POST（如批量文件查询），否则 GET。
async fn curseforge_request(
    app_handle: &tauri::AppHandle,
    client: &reqwest::Client,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let key = curseforge_api_key(app_handle).await;
    let official_url = format!("https://api.curseforge.com/v1/{}", path);

    let k = key.ok_or("未配置 CurseForge API Key，请到 console.curseforge.com 申请后填入设置")?;
    let req = client
        .get(&official_url)
        .header("x-api-key", &k);
    let resp = if let Some(b) = body {
        client
            .post(&official_url)
            .header("x-api-key", &k)
            .json(&b)
            .send()
            .await
            .map_err(|e| format!("官方 API 请求失败: {}", e))?
    } else {
        req.send()
            .await
            .map_err(|e| format!("官方 API 请求失败: {}", e))?
    };
    if !resp.status().is_success() {
        return Err(format!("官方 API HTTP {}", resp.status()));
    }
    resp.json()
        .await
        .map_err(|e| format!("官方 API 解析失败: {}", e))
}

/// 搜索 CurseForge 模组
async fn search_curseforge_mods(
    app_handle: &tauri::AppHandle,
    client: &reqwest::Client,
    query: String,
    loader: String,
    mc_version: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<ModSearchResult>, String> {
    let mut path = format!(
        "mods/search?gameId=432&classId=6&sortField=6&sortOrder=desc&pageSize={}",
        limit.min(50).max(1)
    );
    if !query.trim().is_empty() {
        path.push_str(&format!("&searchFilter={}", url_encode(query.trim())));
    }
    if !loader.is_empty() {
        let lt = curseforge_loader_type(&loader);
        if lt > 0 {
            path.push_str(&format!("&modLoaderType={}", lt));
        }
    }
    if !mc_version.is_empty() {
        path.push_str(&format!("&gameVersion={}", url_encode(&mc_version)));
    }
    if offset > 0 {
        path.push_str(&format!("&index={}", offset));
    }

    let json = curseforge_request(app_handle, client, &path, None).await?;

    let mut results = Vec::new();
    if let Some(data) = json["data"].as_array() {
        for m in data {
            // 汇总最新文件支持的 MC 版本
            let mut game_versions: Vec<String> = Vec::new();
            if let Some(files) = m["latestFiles"].as_array() {
                for f in files {
                    if let Some(gvs) = f["gameVersions"].as_array() {
                        for gv in gvs {
                            if let Some(s) = gv.as_str() {
                                let s = s.to_string();
                                if !game_versions.contains(&s) {
                                    game_versions.push(s);
                                }
                            }
                        }
                    }
                }
            }
            let icon_url = m["logo"]["thumbnailUrl"]
                .as_str()
                .or_else(|| m["logo"]["url"].as_str())
                .unwrap_or("")
                .to_string();
            results.push(ModSearchResult {
                id: m["id"].as_u64().map(|v| v.to_string()).unwrap_or_default(),
                name: m["name"].as_str().unwrap_or("").to_string(),
                summary: m["summary"].as_str().unwrap_or("").to_string(),
                icon_url,
                source: "curseforge".to_string(),
                loaders: Vec::new(), // CF 搜索结果不直接提供加载器列表
                game_versions,
                downloads: m["downloadCount"].as_u64().unwrap_or(0),
                updated_at: m["dateModified"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Ok(results)
}

/// 流式下载模组文件并校验大小 + SHA1
async fn download_mod_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    timeout: Option<u64>,
) -> Result<(), String> {
    // 关键：模组下载客户端与"游戏文件下载"（一直正常）保持完全一致的网络特征：
    // 浏览器 UA + 禁用压缩（Accept-Encoding: identity）+ HTTP/1.1 + 无代理。
    // 某些网络中间设备会按客户端特征区别处理大文件流量，特征不一致正是模组
    // 下载被篡改而游戏下载正常的原因。
    let mut builder = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::ACCEPT_ENCODING,
                reqwest::header::HeaderValue::from_static("identity"),
            );
            h
        })
        .connect_timeout(Duration::from_secs(10))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_proxy()
        .http1_only();
    if let Some(t) = timeout {
        builder = builder.timeout(Duration::from_secs(t));
    }
    let download_client = builder
        .build()
        .map_err(|e| format!("创建下载客户端失败: {}", e))?;

    let resp = download_client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载模组文件失败 [{}]: {}", url, e))?;
    if !resp.status().is_success() {
        return Err(format!("下载模组文件失败 [{}]: HTTP {}", url, resp.status()));
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let mut file = fs::File::create(dest).map_err(|e| format!("创建模组文件失败: {}", e))?;
    let mut stream = resp.bytes_stream();
    let mut hasher = sha1::Sha1::new();
    let mut total_bytes: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载模组数据失败 [{}]: {}", url, e))?;
        total_bytes += chunk.len() as u64;
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|e| format!("写入模组文件失败: {}", e))?;
    }
    file.flush().map_err(|e| e.to_string())?;
    let _ = client; // 保持接口兼容，下载使用独立客户端

    // 大小校验：字节数不符说明下载被截断/篡改，直接判定失败换下一个源
    if let Some(size) = expected_size {
        if total_bytes != size {
            let bad_path = dest.with_extension("jar.bad");
            let _ = fs::rename(dest, &bad_path);
            return Err(format!(
                "模组文件大小不符（预期 {} 字节，实际 {} 字节）[{}]，坏文件保留在 {:?}",
                size, total_bytes, url, bad_path
            ));
        }
    }

    if let Some(sha1) = expected_sha1 {
        // finalize() 返回的就是 20 字节 SHA1，直接转 hex 即可。
        // 注意不能复用 sha1_hex()（它会对输入再算一次 SHA1，造成双重哈希）。
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(sha1) {
            let bad_path = dest.with_extension("jar.bad");
            let _ = fs::rename(dest, &bad_path);
            return Err(format!(
                "模组文件 SHA1 校验失败（预期 {}，实际 {}）[{}]，坏文件保留在 {:?}",
                sha1, actual, url, bad_path
            ));
        }
    }
    // 校验通过：清理可能残留的历史坏文件
    let _ = fs::remove_file(dest.with_extension("jar.bad"));
    Ok(())
}

/// 解析出的模组文件信息（主模组或依赖统一用这套逻辑）
struct ResolvedMod {
    file_name: String,
    sha1: String,
    size: Option<u64>,
    download_sources: Vec<String>,
    /// required 依赖：(来源, 依赖项目 key) —— Modrinth 用 project_id，CurseForge 用 modId
    required_deps: Vec<(String, String)>,
}

/// 查询某模组在「指定加载器 + MC 版本」下的适配文件（新→旧），并收集 required 依赖。
async fn resolve_mod_files(
    app_handle: &tauri::AppHandle,
    client: &reqwest::Client,
    project_id: &str,
    version_name: &str,
    loader: &str,
    source: &str,
) -> Result<ResolvedMod, String> {
    let mut resolved = ResolvedMod {
        file_name: String::new(),
        sha1: String::new(),
        size: None,
        download_sources: Vec::new(),
        required_deps: Vec::new(),
    };

    if source == "curseforge" {
        // CurseForge：查询适配版本 + 加载器的文件列表（新→旧）
        let lt = curseforge_loader_type(loader);
        let path = format!(
            "mods/{}/files?gameVersion={}&modLoaderType={}&pageSize=50",
            url_encode(project_id),
            url_encode(version_name),
            lt
        );
        let json = curseforge_request(app_handle, client, &path, None)
            .await
            .map_err(|e| format!("查询 CurseForge 模组文件失败: {}", e))?;
        let files = json["data"]
            .as_array()
            .ok_or("无效的 CurseForge 文件响应")?;
        if files.is_empty() {
            return Err(format!(
                "该模组没有适配 {} {} 的版本",
                loader, version_name
            ));
        }
        let file = &files[0];
        let file_id = file["id"].as_u64().unwrap_or(0);
        resolved.file_name = file["fileName"].as_str().unwrap_or("").to_string();
        resolved.size = file["fileLength"].as_u64();
        resolved.sha1 = file["hashes"]
            .as_array()
            .and_then(|hs| {
                hs.iter()
                    .find(|h| h["algo"].as_u64() == Some(1))
                    .and_then(|h| h["value"].as_str().map(String::from))
            })
            .unwrap_or_default();
        if resolved.file_name.is_empty() {
            return Err("该模组版本没有可下载的文件".to_string());
        }
        // 收集 required 依赖（relationType == 1 = required）
        if let Some(deps) = file["dependencies"].as_array() {
            for d in deps {
                if d["relationType"].as_u64() == Some(1) {
                    if let Some(mid) = d["modId"].as_u64() {
                        resolved.required_deps.push(("curseforge".to_string(), mid.to_string()));
                    }
                }
            }
        }
        // 下载源：仅官方
        let official = file["downloadUrl"].as_str().unwrap_or("");
        if official.is_empty() {
            let id_str = file_id.to_string();
            let (first4, rest) = if id_str.len() > 4 {
                (&id_str[..4], id_str[4..].trim_start_matches('0'))
            } else {
                (id_str.as_str(), "0")
            };
            let rest = if rest.is_empty() { "0" } else { rest };
            let edge = format!("https://edge.forgecdn.net/files/{}/{}/{}", first4, rest, resolved.file_name);
            resolved.download_sources.push(edge.clone());
            let mediafilez = edge.replace("edge.forgecdn.net", "mediafilez.forgecdn.net");
            resolved.download_sources.push(mediafilez);
        } else {
            resolved.download_sources.push(official.to_string());
        }
    } else {
        // Modrinth：查询适配该版本 + 加载器的模组版本（新→旧）
        let url = format!(
            "https://api.modrinth.com/v2/project/{}/version?loaders={}&game_versions={}",
            url_encode(project_id),
            url_encode(&format!("[\"{}\"]", loader)),
            url_encode(&format!("[\"{}\"]", version_name))
        );
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("查询模组版本失败: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("查询模组版本失败: HTTP {}", resp.status()));
        }
        let versions: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析模组版本失败: {}", e))?;
        let versions_array = versions.as_array().ok_or("无效的模组版本响应")?;
        if versions_array.is_empty() {
            return Err(format!(
                "该模组没有适配 {} {} 的版本",
                loader, version_name
            ));
        }
        // 选最新版本的主文件（primary）
        for ver in versions_array {
            if let Some(files) = ver["files"].as_array() {
                let primary = files
                    .iter()
                    .find(|f| f["primary"].as_bool() == Some(true))
                    .or_else(|| files.first());
                if let Some(file) = primary {
                    resolved.file_name = file["filename"].as_str().unwrap_or("").to_string();
                    resolved.sha1 = file["hashes"]["sha1"].as_str().unwrap_or("").to_string();
                    resolved.size = file["size"].as_u64();
                    let file_url = file["url"].as_str().unwrap_or("").to_string();
                    if !file_url.is_empty() {
                        // 官方链路多路下载：官方 cdn、官方+缓存击穿、cdn-alt、cdn-alt+击穿
                        let cache_bust = format!("?lumia={}", std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis());
                        let alt_url = file_url
                            .replace("https://cdn.modrinth.com", "https://cdn-alt.modrinth.com")
                            .replace("https://cdn-raw.modrinth.com", "https://cdn-alt.modrinth.com");
                        resolved.download_sources.push(file_url.clone());
                        resolved.download_sources.push(format!("{}{}", file_url, cache_bust));
                        resolved.download_sources.push(alt_url.clone());
                        resolved.download_sources.push(format!("{}{}", alt_url, cache_bust));
                    }
                    // 收集 required 依赖（dependency_type == "required"）
                    if let Some(deps) = ver["dependencies"].as_array() {
                        for d in deps {
                            if d["dependency_type"].as_str() == Some("required") {
                                if let Some(pid) = d["project_id"].as_str() {
                                    if !pid.is_empty() {
                                        resolved.required_deps.push(("modrinth".to_string(), pid.to_string()));
                                    }
                                }
                            }
                        }
                    }
                    break;
                }
            }
        }
        if resolved.file_name.is_empty() || resolved.download_sources.is_empty() {
            return Err("该模组版本没有可下载的文件".to_string());
        }
    }
    Ok(resolved)
}

/// 依次尝试下载源（官方/击穿/直连/镜像多路），SHA1 校验
async fn download_mod_with_fallback(
    client: &reqwest::Client,
    sources: &[String],
    dest: &Path,
    sha1_opt: Option<&str>,
    size_opt: Option<u64>,
) -> Result<(), String> {
    let max_attempts = sources.len();
    let mut attempts = 0usize;
    loop {
        let src = &sources[attempts];
        attempts += 1;
        dl_log(&format!("模组下载尝试 {}/{}: {}", attempts, max_attempts, src));
        match download_mod_file(client, src, dest, sha1_opt, size_opt, None).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                dl_log(&format!("模组下载源失败: {}", e));
                if attempts >= max_attempts {
                    return Err(e);
                }
                sleep(Duration::from_millis(800)).await;
            }
        }
    }
}

/// 递归安装模组依赖（visited 防环；目标文件已存在则跳过视为已装）。
/// 用显式 Stack 迭代代替 async 递归（Rust async fn 直接递归不安全）。
async fn install_mod_dependencies(
    app_handle: &tauri::AppHandle,
    client: &reqwest::Client,
    game_dir: &Path,
    version_name: &str,
    loader: &str,
    project_id: &str,
    source: &str,
    visited: &mut std::collections::HashSet<String>,
) -> Result<Vec<String>, String> {
    // DFS 栈：待解析的 (source, project_id)
    let mut stack: Vec<(String, String)> = vec![(source.to_string(), project_id.to_string())];
    let mut installed: Vec<String> = Vec::new();

    while let Some((src, pid)) = stack.pop() {
        let key = format!("{}:{}", src, pid);
        if !visited.insert(key) {
            continue;
        }
        let resolved = match resolve_mod_files(app_handle, client, &pid, version_name, loader, &src).await {
            Ok(r) => r,
            Err(e) => {
                dl_log(&format!("依赖 {} 解析失败，跳过: {}", pid, e));
                continue;
            }
        };
        let mods_dir = game_dir.join("versions").join(version_name).join("mods");
        let dest = mods_dir.join(&resolved.file_name);
        if dest.exists() {
            dl_log(&format!("依赖模组已存在，跳过: {}", resolved.file_name));
        } else {
            let sha1_opt = if resolved.sha1.is_empty() {
                None
            } else {
                Some(resolved.sha1.as_str())
            };
            if let Err(e) = download_mod_with_fallback(client, &resolved.download_sources, &dest, sha1_opt, resolved.size).await {
                dl_log(&format!("依赖模组 {} 下载失败，跳过: {}", resolved.file_name, e));
            } else {
                installed.push(resolved.file_name.clone());
                dl_log(&format!("已安装依赖模组: {}", resolved.file_name));
            }
        }
        // 把该依赖自己的 required 依赖压栈（后处理 = 深度优先）
        for (dep_source, dep_id) in resolved.required_deps {
            stack.push((dep_source, dep_id));
        }
    }
    Ok(installed)
}

/// 批量查询依赖项目的显示名称（用于前端提示「将顺带安装依赖」）
async fn dependency_display_names(
    app_handle: &tauri::AppHandle,
    client: &reqwest::Client,
    deps: &[(String, String)],
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    // Modrinth：批量 GET /v2/projects?ids=[...]
    let mr_ids: Vec<String> = deps
        .iter()
        .filter(|(s, _)| s == "modrinth")
        .map(|(_, id)| id.clone())
        .collect();
    if !mr_ids.is_empty() {
        let ids_json = serde_json::to_string(&mr_ids).unwrap_or_default();
        let url = format!(
            "https://api.modrinth.com/v2/projects?ids={}",
            url_encode(&ids_json)
        );
        if let Ok(resp) = client.get(&url).send().await {
            if resp.status().is_success() {
                if let Ok(arr) = resp.json::<serde_json::Value>().await {
                    if let Some(items) = arr.as_array() {
                        for it in items {
                            if let Some(title) = it["title"].as_str() {
                                names.push(title.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    // CurseForge：POST /v1/mods {"modIds":[...]}
    let cf_ids: Vec<u64> = deps
        .iter()
        .filter(|(s, _)| s == "curseforge")
        .filter_map(|(_, id)| id.parse().ok())
        .collect();
    if !cf_ids.is_empty() {
        let body = serde_json::json!({ "modIds": cf_ids });
        if let Ok(json) = curseforge_request(app_handle, client, "mods", Some(body)).await {
            if let Some(items) = json["data"].as_array() {
                for it in items {
                    if let Some(name) = it["name"].as_str() {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }
    names
}

/// 检查模组对某本地版本是否可用（无加载器 / 无适配版本 → 不支持），并返回 required 依赖名。
#[tauri::command]
async fn check_mod_support(
    app_handle: tauri::AppHandle,
    project_id: String,
    version_name: String,
    source: String,
) -> Result<ModSupportInfo, String> {
    let game_dir = get_game_dir(&app_handle).await;
    // 检测目标版本加载器
    let json_path = game_dir
        .join("versions")
        .join(&version_name)
        .join(format!("{}.json", version_name));
    let json_str =
        fs::read_to_string(&json_path).map_err(|e| format!("读取版本 json 失败: {}", e))?;
    let version_json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("解析版本 json 失败: {}", e))?;
    let loader = detect_loader_from_json(&version_json);
    let loader_param = match loader.as_str() {
        "fabric" | "forge" | "neoforge" | "quilt" => loader.clone(),
        _ => {
            return Ok(ModSupportInfo {
                supported: false,
                reason: Some(format!(
                    "版本 {} 没有安装模组加载器，原版无法加载模组",
                    version_name
                )),
                dependencies: vec![],
            })
        }
    };

    let client = modrinth_client()?;
    // 查询适配版本：失败即视为不支持，附原因
    match resolve_mod_files(&app_handle, &client, &project_id, &version_name, &loader_param, &source).await {
        Ok(resolved) => {
            let dep_names = dependency_display_names(&app_handle, &client, &resolved.required_deps).await;
            Ok(ModSupportInfo {
                supported: true,
                reason: None,
                dependencies: dep_names,
            })
        }
        Err(e) => Ok(ModSupportInfo {
            supported: false,
            reason: Some(e),
            dependencies: vec![],
        }),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModSupportInfo {
    supported: bool,
    reason: Option<String>,
    dependencies: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModInstallResult {
    file_name: String,
    dependencies: Vec<String>,
}

/// 安装模组：检测加载器 → 解析适配文件 → 下载主模组 → 顺带递归安装 required 依赖
#[tauri::command]
async fn install_mod(
    app_handle: tauri::AppHandle,
    project_id: String,
    version_name: String,
    source: String,
) -> Result<ModInstallResult, String> {
    let game_dir = get_game_dir(&app_handle).await;

    // 1. 检测目标版本的加载器
    let json_path = game_dir
        .join("versions")
        .join(&version_name)
        .join(format!("{}.json", version_name));
    let json_str =
        fs::read_to_string(&json_path).map_err(|e| format!("读取版本 json 失败: {}", e))?;
    let version_json: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("解析版本 json 失败: {}", e))?;
    let loader = detect_loader_from_json(&version_json);
    let loader_param = match loader.as_str() {
        "fabric" | "forge" | "neoforge" | "quilt" => loader.clone(),
        _ => {
            return Err(format!(
                "版本 {} 没有安装模组加载器，原版无法加载模组",
                version_name
            ))
        }
    };

    let client = modrinth_client()?;

    // 2. 解析主模组的适配文件（无适配版本 -> 后端报错；前端已用 check_mod_support 先拦截）
    let resolved = resolve_mod_files(
        &app_handle,
        &client,
        &project_id,
        &version_name,
        &loader_param,
        &source,
    )
    .await?;

    // 3. 下载主模组到 mods 目录（多路源 + SHA1 校验）
    let mods_dir = game_dir
        .join("versions")
        .join(&version_name)
        .join("mods");
    let dest = mods_dir.join(&resolved.file_name);
    let sha1_opt = if resolved.sha1.is_empty() {
        None
    } else {
        Some(resolved.sha1.as_str())
    };
    download_mod_with_fallback(&client, &resolved.download_sources, &dest, sha1_opt, resolved.size)
        .await?;

    // 4. 顺带安装 required 依赖（递归，防环；已有文件跳过）
    let mut visited = std::collections::HashSet::new();
    // 主模组自身入 visited，防止依赖回指时重复展开
    visited.insert(format!("{}:{}", source, project_id));
    let mut dependencies: Vec<String> = Vec::new();
    for (dep_source, dep_id) in &resolved.required_deps {
        match install_mod_dependencies(
            &app_handle,
            &client,
            &game_dir,
            &version_name,
            &loader_param,
            dep_id,
            dep_source,
            &mut visited,
        )
        .await
        {
            Ok(mut names) => dependencies.append(&mut names),
            Err(e) => {
                dl_log(&format!("依赖 {} 安装失败，继续安装其它: {}", dep_id, e));
            }
        }
    }

    Ok(ModInstallResult {
        file_name: resolved.file_name.clone(),
        dependencies,
    })
}

// ========== 3.10b 资源包下载（原版即可用，无 loader 维度） ==========

/// 搜索资源包：Modrinth project_type=resourcepack，按 MC 版本过滤（资源包不依赖加载器）
#[tauri::command]
async fn search_resource_packs(
    query: String,
    mc_version: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<ModSearchResult>, String> {
    let client = modrinth_client()?;

    // facets：外层 OR、内层 AND；过滤资源包类型 + MC 版本（无 loader）
    let mut facets: Vec<Vec<String>> = vec![vec!["project_type:resourcepack".to_string()]];
    if !mc_version.is_empty() {
        facets.push(vec![format!("versions:{}", mc_version)]);
    }

    let facets_json = serde_json::to_string(&facets).unwrap_or_default();
    let url = format!(
        "https://api.modrinth.com/v2/search?query={}&facets={}&index=downloads&offset={}&limit={}",
        url_encode(&query),
        url_encode(&facets_json),
        offset,
        limit.min(100).max(1)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Modrinth 搜索请求失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Modrinth 搜索失败: HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析搜索结果失败: {}", e))?;

    let mut results = Vec::new();
    if let Some(hits) = json["hits"].as_array() {
        for hit in hits {
            let game_versions: Vec<String> = hit["versions"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            results.push(ModSearchResult {
                id: hit["project_id"].as_str().unwrap_or("").to_string(),
                name: hit["title"].as_str().unwrap_or("").to_string(),
                summary: hit["description"].as_str().unwrap_or("").to_string(),
                icon_url: hit["icon_url"].as_str().unwrap_or("").to_string(),
                source: "modrinth".to_string(),
                loaders: Vec::new(), // 资源包无加载器维度
                game_versions,
                downloads: hit["downloads"].as_u64().unwrap_or(0),
                updated_at: hit["date_modified"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Ok(results)
}

/// 解析资源包在当前 MC 版本下的适配文件（Modrinth version API，只按 game_versions 过滤）
async fn resolve_resource_pack_files(
    client: &reqwest::Client,
    project_id: &str,
    version_name: &str,
) -> Result<ResolvedMod, String> {
    let mut resolved = ResolvedMod {
        file_name: String::new(),
        sha1: String::new(),
        size: None,
        download_sources: Vec::new(),
        required_deps: Vec::new(),
    };

    // Modrinth：查询适配该版本号的版本（新→旧），加载器维度传空（资源包不需要）
    let url = format!(
        "https://api.modrinth.com/v2/project/{}/version?game_versions={}",
        url_encode(project_id),
        url_encode(&format!("[\"{}\"]", version_name))
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("查询资源包版本失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("查询资源包版本失败: HTTP {}", resp.status()));
    }
    let versions: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析资源包版本失败: {}", e))?;
    let versions_array = versions.as_array().ok_or("无效的资源包版本响应")?;
    if versions_array.is_empty() {
        return Err(format!("该资源包没有适配 {} 的版本", version_name));
    }
    // 选最新版本的主文件（primary）
    for ver in versions_array {
        if let Some(files) = ver["files"].as_array() {
            let primary = files
                .iter()
                .find(|f| f["primary"].as_bool() == Some(true))
                .or_else(|| files.first());
            if let Some(file) = primary {
                resolved.file_name = file["filename"].as_str().unwrap_or("").to_string();
                resolved.sha1 = file["hashes"]["sha1"].as_str().unwrap_or("").to_string();
                resolved.size = file["size"].as_u64();
                let file_url = file["url"].as_str().unwrap_or("").to_string();
                if !file_url.is_empty() {
                    // 官方链路多路下载：官方 cdn、官方+缓存击穿、cdn-alt、cdn-alt+击穿
                    let cache_bust = format!("?lumia={}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis());
                    let alt_url = file_url
                        .replace("https://cdn.modrinth.com", "https://cdn-alt.modrinth.com")
                        .replace("https://cdn-raw.modrinth.com", "https://cdn-alt.modrinth.com");
                    resolved.download_sources.push(file_url.clone());
                    resolved.download_sources.push(format!("{}{}", file_url, cache_bust));
                    resolved.download_sources.push(alt_url.clone());
                    resolved.download_sources.push(format!("{}{}", alt_url, cache_bust));
                }
            }
        }
        if !resolved.file_name.is_empty() {
            break;
        }
    }
    if resolved.file_name.is_empty() || resolved.download_sources.is_empty() {
        return Err("该资源包版本没有可下载的文件".to_string());
    }
    Ok(resolved)
}

/// 安装资源包：解析适配文件 → 多路下载落盘到 versions/<版本>/resourcepacks/（原版即可用，无依赖）
#[tauri::command]
async fn install_resource_pack(
    app_handle: tauri::AppHandle,
    project_id: String,
    version_name: String,
) -> Result<ModInstallResult, String> {
    let game_dir = get_game_dir(&app_handle).await;

    // 校验目标版本存在
    let version_dir = game_dir.join("versions").join(&version_name);
    if !version_dir.exists() {
        return Err(format!("版本 {} 不存在", version_name));
    }

    let client = modrinth_client()?;

    // 解析适配文件（无 loader 维度）
    let resolved = resolve_resource_pack_files(&client, &project_id, &version_name).await?;

    // 下载到 resourcepacks 目录（多路源 + SHA1 校验）
    let rp_dir = version_dir.join("resourcepacks");
    let dest = rp_dir.join(&resolved.file_name);
    let sha1_opt = if resolved.sha1.is_empty() {
        None
    } else {
        Some(resolved.sha1.as_str())
    };
    download_mod_with_fallback(&client, &resolved.download_sources, &dest, sha1_opt, resolved.size).await?;

    Ok(ModInstallResult {
        file_name: resolved.file_name.clone(),
        dependencies: Vec::new(),
    })
}

// ========== 3.10c 光影包下载（原版即可用，无 loader 维度） ==========

/// 搜索光影包：Modrinth project_type=shader，按 MC 版本过滤（光影包不依赖加载器）
#[tauri::command]
async fn search_shader_packs(
    query: String,
    mc_version: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<ModSearchResult>, String> {
    let client = modrinth_client()?;

    // facets：外层 OR、内层 AND；过滤光影包类型 + MC 版本（无 loader）
    let mut facets: Vec<Vec<String>> = vec![vec!["project_type:shader".to_string()]];
    if !mc_version.is_empty() {
        facets.push(vec![format!("versions:{}", mc_version)]);
    }

    let facets_json = serde_json::to_string(&facets).unwrap_or_default();
    let url = format!(
        "https://api.modrinth.com/v2/search?query={}&facets={}&index=downloads&offset={}&limit={}",
        url_encode(&query),
        url_encode(&facets_json),
        offset,
        limit.min(100).max(1)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Modrinth 搜索请求失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Modrinth 搜索失败: HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析搜索结果失败: {}", e))?;

    let mut results = Vec::new();
    if let Some(hits) = json["hits"].as_array() {
        for hit in hits {
            let game_versions: Vec<String> = hit["versions"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            results.push(ModSearchResult {
                id: hit["project_id"].as_str().unwrap_or("").to_string(),
                name: hit["title"].as_str().unwrap_or("").to_string(),
                summary: hit["description"].as_str().unwrap_or("").to_string(),
                icon_url: hit["icon_url"].as_str().unwrap_or("").to_string(),
                source: "modrinth".to_string(),
                loaders: hit["loaders"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                game_versions,
                downloads: hit["downloads"].as_u64().unwrap_or(0),
                updated_at: hit["date_modified"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Ok(results)
}

/// 解析光影包在当前 MC 版本下的适配文件（Modrinth version API，只按 game_versions 过滤）
async fn resolve_shader_pack_files(
    client: &reqwest::Client,
    project_id: &str,
    version_name: &str,
) -> Result<ResolvedMod, String> {
    let mut resolved = ResolvedMod {
        file_name: String::new(),
        sha1: String::new(),
        size: None,
        download_sources: Vec::new(),
        required_deps: Vec::new(),
    };

    // Modrinth：查询适配该版本号的版本（新→旧），加载器维度传空（光影包不需要）
    let url = format!(
        "https://api.modrinth.com/v2/project/{}/version?game_versions={}",
        url_encode(project_id),
        url_encode(&format!("[\"{}\"]", version_name))
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("查询光影包版本失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("查询光影包版本失败: HTTP {}", resp.status()));
    }
    let versions: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析光影包版本失败: {}", e))?;
    let versions_array = versions.as_array().ok_or("无效的光影包版本响应")?;
    if versions_array.is_empty() {
        return Err(format!("该光影包没有适配 {} 的版本", version_name));
    }
    // 选最新版本的主文件（primary）
    for ver in versions_array {
        if let Some(files) = ver["files"].as_array() {
            let primary = files
                .iter()
                .find(|f| f["primary"].as_bool() == Some(true))
                .or_else(|| files.first());
            if let Some(file) = primary {
                resolved.file_name = file["filename"].as_str().unwrap_or("").to_string();
                resolved.sha1 = file["hashes"]["sha1"].as_str().unwrap_or("").to_string();
                resolved.size = file["size"].as_u64();
                let file_url = file["url"].as_str().unwrap_or("").to_string();
                if !file_url.is_empty() {
                    // 官方链路多路下载：官方 cdn、官方+缓存击穿、cdn-alt、cdn-alt+击穿
                    let cache_bust = format!("?lumia={}", std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis());
                    let alt_url = file_url
                        .replace("https://cdn.modrinth.com", "https://cdn-alt.modrinth.com")
                        .replace("https://cdn-raw.modrinth.com", "https://cdn-alt.modrinth.com");
                    resolved.download_sources.push(file_url.clone());
                    resolved.download_sources.push(format!("{}{}", file_url, cache_bust));
                    resolved.download_sources.push(alt_url.clone());
                    resolved.download_sources.push(format!("{}{}", alt_url, cache_bust));
                }
            }
        }
        if !resolved.file_name.is_empty() {
            break;
        }
    }
    if resolved.file_name.is_empty() || resolved.download_sources.is_empty() {
        return Err("该光影包版本没有可下载的文件".to_string());
    }
    Ok(resolved)
}

/// 安装光影包：解析适配文件 → 多路下载落盘到 versions/<版本>/shaderpacks/（原版即可用，无依赖）
#[tauri::command]
async fn install_shader_pack(
    app_handle: tauri::AppHandle,
    project_id: String,
    version_name: String,
) -> Result<ModInstallResult, String> {
    let game_dir = get_game_dir(&app_handle).await;

    // 校验目标版本存在
    let version_dir = game_dir.join("versions").join(&version_name);
    if !version_dir.exists() {
        return Err(format!("版本 {} 不存在", version_name));
    }

    let client = modrinth_client()?;

    // 解析适配文件（无 loader 维度）
    let resolved = resolve_shader_pack_files(&client, &project_id, &version_name).await?;

    // 下载到 shaderpacks 目录（多路源 + SHA1 校验）
    let shader_dir = version_dir.join("shaderpacks");
    let dest = shader_dir.join(&resolved.file_name);
    let sha1_opt = if resolved.sha1.is_empty() {
        None
    } else {
        Some(resolved.sha1.as_str())
    };
    download_mod_with_fallback(&client, &resolved.download_sources, &dest, sha1_opt, resolved.size).await?;

    Ok(ModInstallResult {
        file_name: resolved.file_name.clone(),
        dependencies: Vec::new(),
    })
}

// ========== 3.11 版本管理 ==========

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModFileInfo {
    file_name: String,
    display_name: String,
    enabled: bool,
    size: u64,
}

/// 重命名版本：目录、json、jar 同步改名，并更新其他版本的 inheritsFrom 引用
#[tauri::command]
async fn rename_version(
    app_handle: tauri::AppHandle,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    let game_dir = get_game_dir(&app_handle).await;
    let versions_root = game_dir.join("versions");
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        return Err("新名称不能为空".to_string());
    }
    if new_name.contains('/') || new_name.contains('\\') || new_name.contains(':') {
        return Err("名称不能包含 / \\ : 字符".to_string());
    }
    if old_name == new_name {
        return Ok(());
    }
    let old_dir = versions_root.join(&old_name);
    let new_dir = versions_root.join(&new_name);
    if !old_dir.exists() {
        return Err(format!("版本 {} 不存在", old_name));
    }
    if new_dir.exists() {
        return Err(format!("名称 {} 已存在", new_name));
    }

    fs::rename(&old_dir, &new_dir).map_err(|e| format!("重命名目录失败: {}", e))?;

    // json 改名并更新 id 字段
    let old_json = new_dir.join(format!("{}.json", old_name));
    let new_json = new_dir.join(format!("{}.json", new_name));
    if old_json.exists() {
        let text = fs::read_to_string(&old_json).map_err(|e| e.to_string())?;
        let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        v["id"] = serde_json::json!(new_name);
        fs::write(&new_json, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::remove_file(&old_json).map_err(|e| e.to_string())?;
    }

    // jar 改名
    let old_jar = new_dir.join(format!("{}.jar", old_name));
    let new_jar = new_dir.join(format!("{}.jar", new_name));
    if old_jar.exists() {
        fs::rename(&old_jar, &new_jar).map_err(|e| e.to_string())?;
    }

    // 其他版本 json 中对该版本的 inheritsFrom 引用同步更新
    if let Ok(entries) = fs::read_dir(&versions_root) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            if name == new_name {
                continue;
            }
            let jp = dir.join(format!("{}.json", name));
            if !jp.exists() {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&jp) {
                if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if v["inheritsFrom"].as_str() == Some(old_name.as_str()) {
                        v["inheritsFrom"] = serde_json::json!(new_name);
                        let _ = fs::write(&jp, serde_json::to_string_pretty(&v).unwrap_or(text));
                    }
                }
            }
        }
    }
    Ok(())
}

/// 删除版本目录
#[tauri::command]
async fn delete_version(app_handle: tauri::AppHandle, name: String) -> Result<(), String> {
    let game_dir = get_game_dir(&app_handle).await;
    let dir = game_dir.join("versions").join(&name);
    if !dir.exists() {
        return Err(format!("版本 {} 不存在", name));
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("删除版本失败: {}", e))
}

/// 列出某版本 mods 目录下的模组（.jar / .jar.disabled）
#[tauri::command]
async fn list_mods(app_handle: tauri::AppHandle, version: String) -> Result<Vec<ModFileInfo>, String> {
    let game_dir = get_game_dir(&app_handle).await;
    let mods_dir = game_dir.join("versions").join(&version).join("mods");
    let mut mods = Vec::new();
    if let Ok(entries) = fs::read_dir(&mods_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let fname = entry.file_name().to_string_lossy().to_string();
            let enabled = !fname.ends_with(".disabled");
            if enabled && !fname.ends_with(".jar") {
                continue;
            }
            let display_name = fname
                .trim_end_matches(".disabled")
                .trim_end_matches(".jar")
                .to_string();
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            mods.push(ModFileInfo {
                file_name: fname,
                display_name,
                enabled,
                size,
            });
        }
    }
    mods.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
    Ok(mods)
}

/// 启用/禁用模组核心逻辑（.jar ↔ .jar.disabled 重命名，与主流启动器一致；独立函数便于单测）
fn toggle_mod_file(mods_dir: &Path, file_name: &str, enable: bool) -> Result<(), String> {
    let base = file_name.trim_end_matches(".disabled").to_string();
    let jar_path = mods_dir.join(&base);
    let disabled_path = mods_dir.join(format!("{}.disabled", base));
    if enable {
        if disabled_path.exists() && !jar_path.exists() {
            fs::rename(&disabled_path, &jar_path).map_err(|e| {
                format!("启用模组失败（若游戏正在运行，请先关闭游戏）: {}", e)
            })?;
        }
    } else if jar_path.exists() && !disabled_path.exists() {
        fs::rename(&jar_path, &disabled_path).map_err(|e| {
            format!("禁用模组失败（若游戏正在运行，请先关闭游戏）: {}", e)
        })?;
    }
    Ok(())
}

/// 删除模组核心逻辑（同时清理 .jar 与 .jar.disabled）
fn delete_mod_file(mods_dir: &Path, file_name: &str) -> Result<(), String> {
    let base = file_name.trim_end_matches(".disabled");
    for p in [
        mods_dir.join(base),
        mods_dir.join(format!("{}.disabled", base)),
    ] {
        if p.exists() {
            fs::remove_file(&p).map_err(|e| {
                format!("删除模组失败（若游戏正在运行，请先关闭游戏）: {}", e)
            })?;
        }
    }
    Ok(())
}

/// 启用/禁用模组（.jar ↔ .jar.disabled 重命名，与主流启动器一致）
#[tauri::command]
async fn toggle_mod(
    app_handle: tauri::AppHandle,
    version: String,
    file_name: String,
    enabled: bool,
) -> Result<(), String> {
    let game_dir = get_game_dir(&app_handle).await;
    let mods_dir = game_dir.join("versions").join(&version).join("mods");
    toggle_mod_file(&mods_dir, &file_name, enabled)
}

/// 删除模组文件
#[tauri::command]
async fn delete_mod(
    app_handle: tauri::AppHandle,
    version: String,
    file_name: String,
) -> Result<(), String> {
    let game_dir = get_game_dir(&app_handle).await;
    let mods_dir = game_dir.join("versions").join(&version).join("mods");
    delete_mod_file(&mods_dir, &file_name)
}

/// 在文件管理器中打开某版本的 mods 文件夹
#[tauri::command]
async fn open_mods_folder(app_handle: tauri::AppHandle, version: String) -> Result<(), String> {
    let game_dir = get_game_dir(&app_handle).await;
    let mods_dir = game_dir.join("versions").join(&version).join("mods");
    fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    open::that(&mods_dir).map_err(|e| format!("打开文件夹失败: {}", e))
}

// ========== 3.13 整合包安装（Modrinth .mrpack / CurseForge .zip） ==========

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ModpackInfo {
    format: String, // "modrinth" | "curseforge"
    name: String,
    mc_version: String,
    loader: String,
    loader_version: String,
    file_count: usize,
}

fn open_pack_archive(path: &str) -> Result<zip::ZipArchive<fs::File>, String> {
    let file = fs::File::open(path).map_err(|e| format!("无法打开整合包文件: {}", e))?;
    zip::ZipArchive::new(file).map_err(|e| format!("不是有效的 zip 压缩包: {}", e))
}

fn read_pack_file(archive: &mut zip::ZipArchive<fs::File>, name: &str) -> Option<String> {
    archive
        .by_name(name)
        .ok()
        .and_then(|mut f| {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut f, &mut s)
                .ok()
                .map(|_| s)
        })
}

fn parse_modpack_index(path: &str) -> Result<ModpackInfo, String> {
    let mut archive = open_pack_archive(path)?;

    // Modrinth 整合包：modrinth.index.json
    if let Some(index_text) = read_pack_file(&mut archive, "modrinth.index.json") {
        let j: serde_json::Value =
            serde_json::from_str(&index_text).map_err(|e| format!("解析 modrinth.index.json 失败: {}", e))?;
        let name = j["name"].as_str().unwrap_or("未命名整合包").to_string();
        let deps = &j["dependencies"];
        let mc_version = deps["minecraft"].as_str().unwrap_or("").to_string();
        // 加载器优先级：fabric-loader > quilt-loader > neoforge > forge
        let mut loader = String::new();
        let mut loader_version = String::new();
        for (dep, l) in [
            ("fabric-loader", "fabric"),
            ("quilt-loader", "quilt"),
            ("neoforge", "neoforge"),
            ("forge", "forge"),
        ] {
            if let Some(v) = deps[dep].as_str() {
                loader = l.to_string();
                loader_version = v.to_string();
                break;
            }
        }
        let file_count = j["files"].as_array().map(|a| a.len()).unwrap_or(0);
        return Ok(ModpackInfo {
            format: "modrinth".to_string(),
            name,
            mc_version,
            loader,
            loader_version,
            file_count,
        });
    }

    // CurseForge 整合包：manifest.json
    if let Some(manifest_text) = read_pack_file(&mut archive, "manifest.json") {
        let j: serde_json::Value =
            serde_json::from_str(&manifest_text).map_err(|e| format!("解析 manifest.json 失败: {}", e))?;
        let name = j["name"].as_str().unwrap_or("未命名整合包").to_string();
        let mc_version = j["minecraft"]["version"].as_str().unwrap_or("").to_string();
        let mut loader = String::new();
        let mut loader_version = String::new();
        if let Some(loaders) = j["minecraft"]["modLoaders"].as_array() {
            for ml in loaders {
                if let Some(id) = ml["id"].as_str() {
                    // 形如 "forge-47.2.0" / "fabric-0.16.10" / "neoforge-21.1.166"
                    if let Some((l, v)) = id.split_once('-') {
                        let l = l.to_lowercase();
                        if ["forge", "fabric", "neoforge", "quilt"].contains(&l.as_str()) {
                            loader = l;
                            loader_version = v.to_string();
                            break;
                        }
                    }
                }
            }
        }
        let file_count = j["files"].as_array().map(|a| a.len()).unwrap_or(0);
        return Ok(ModpackInfo {
            format: "curseforge".to_string(),
            name,
            mc_version,
            loader,
            loader_version,
            file_count,
        });
    }

    Err("无法识别的整合包格式（需要 modrinth.index.json 或 manifest.json）".to_string())
}

/// 从版本列表中挑选与 spec 匹配的最新版本（spec 支持 "x" 通配前缀，如 "0.16.x"）
fn pick_matching_version(versions: &[String], spec: &str) -> Option<String> {
    let exact = spec.trim().trim_end_matches('x').trim_end_matches('X').trim_end_matches('.');
    let mut matched: Vec<&String> = versions
        .iter()
        .filter(|v| exact.is_empty() || v.starts_with(exact))
        .collect();
    matched.sort_by(|a, b| version_cmp(b, a));
    matched.first().map(|s| s.to_string())
}

/// 把整合包要求的加载器版本规格解析为可安装的确切版本号
async fn resolve_loader_version(
    client: &reqwest::Client,
    mc_version: &str,
    loader: &str,
    spec: &str,
) -> Result<String, String> {
    match loader {
        "fabric" => {
            let url = format!("https://meta.fabricmc.net/v2/versions/loader/{}", mc_version);
            let json: serde_json::Value = client
                .get(&url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            let versions: Vec<String> = json
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e["loader"]["version"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            pick_matching_version(&versions, spec)
                .ok_or_else(|| format!("未找到匹配的 Fabric Loader 版本: {}", spec))
        }
        "quilt" => {
            let url = format!("https://meta.quiltmc.org/v3/versions/loader/{}", mc_version);
            let json: serde_json::Value = client
                .get(&url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            let versions: Vec<String> = json
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e["loader"]["version"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            pick_matching_version(&versions, spec)
                .ok_or_else(|| format!("未找到匹配的 Quilt Loader 版本: {}", spec))
        }
        "forge" => {
            let url = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
            let xml = client
                .get(url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            let mut builds: Vec<String> = Vec::new();
            for line in xml.lines() {
                if line.contains(&format!("<version>{}", mc_version)) {
                    let start = line.find('>').unwrap_or(0) + 1;
                    let end = line.rfind('<').unwrap_or(line.len());
                    let ver = line[start..end].trim();
                    if let Some((_, build)) = ver.split_once('-') {
                        builds.push(build.to_string());
                    }
                }
            }
            let build = pick_matching_version(&builds, spec)
                .ok_or_else(|| format!("未找到匹配的 Forge 版本: {}", spec))?;
            Ok(format!("{}-{}", mc_version, build))
        }
        "neoforge" => {
            let url = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
            let xml = client
                .get(url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            let mut versions: Vec<String> = Vec::new();
            for line in xml.lines() {
                if line.contains("<version>") {
                    let start = line.find('>').unwrap_or(0) + 1;
                    let end = line.rfind('<').unwrap_or(line.len());
                    versions.push(line[start..end].trim().to_string());
                }
            }
            pick_matching_version(&versions, spec)
                .ok_or_else(|| format!("未找到匹配的 NeoForge 版本: {}", spec))
        }
        _ => Ok(String::new()),
    }
}

/// 解压 zip 内指定目录到目标目录（如 overrides/）
fn extract_zip_dir(
    archive: &mut zip::ZipArchive<fs::File>,
    prefix: &str,
    dest_dir: &Path,
) -> Result<(), String> {
    let prefix_with_slash = format!("{}/", prefix);
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| format!("读取压缩包失败: {}", e))?;
        let name = f.name().to_string();
        if !name.starts_with(&prefix_with_slash) {
            continue;
        }
        let rel = &name[prefix_with_slash.len()..];
        if rel.is_empty() {
            continue;
        }
        let out = dest_dir.join(rel);
        if f.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out_file = fs::File::create(&out).map_err(|e| format!("写入覆盖文件失败: {}", e))?;
        std::io::copy(&mut f, &mut out_file).map_err(|e| format!("解压失败: {}", e))?;
    }
    Ok(())
}

/// 识别整合包（拖入后前端先调用此命令显示信息）
#[tauri::command]
async fn inspect_modpack(path: String) -> Result<ModpackInfo, String> {
    parse_modpack_index(&path)
}

/// 安装整合包：安装游戏版本+加载器 → 下载模组文件 → 解压覆盖文件
#[tauri::command]
async fn install_modpack(
    app_handle: tauri::AppHandle,
    window: Window,
    path: String,
    custom_version_name: String,
) -> Result<String, String> {
    let game_dir = get_game_dir(&app_handle).await;
    let info = parse_modpack_index(&path)?;

    if info.mc_version.is_empty() {
        return Err("整合包未声明 Minecraft 版本".to_string());
    }

    let client = modrinth_client()?;

    // 1. 安装游戏版本 + 加载器（复用 download_game，包含进度事件）
    let loader_version = if info.loader.is_empty() {
        String::new()
    } else {
        resolve_loader_version(&client, &info.mc_version, &info.loader, &info.loader_version).await?
    };
    dl_log(&format!(
        "整合包 {}：MC {} / Loader {} {}",
        info.name, info.mc_version, info.loader, loader_version
    ));
    download_game(
        window.clone(),
        app_handle.clone(),
        DownloadConfig {
            version: info.mc_version.clone(),
            display_name: info.name.clone(),
            loader: if info.loader.is_empty() { "none".to_string() } else { info.loader.clone() },
            loader_version,
            install_fabric_api: false,
        },
    )
    .await
    .map_err(|e| format!("安装游戏版本失败: {}", e))?;

    let version_dir = game_dir.join("versions").join(&info.mc_version);

    // 2. 下载模组文件
    let mut archive = open_pack_archive(&path)?;
    if info.format == "modrinth" {
        let index_text = read_pack_file(&mut archive, "modrinth.index.json")
            .ok_or("整合包缺少 modrinth.index.json")?;
        let j: serde_json::Value =
            serde_json::from_str(&index_text).map_err(|e| format!("解析整合包索引失败: {}", e))?;
        let files = j["files"].as_array().cloned().unwrap_or_default();
        let n = files.len();
        for (i, f) in files.iter().enumerate() {
            let rel = f["path"].as_str().ok_or("文件缺少 path 字段")?;
            let sha1 = f["hashes"]["sha1"].as_str().map(String::from);
            let dest = version_dir.join(rel);
            let downloads = f["downloads"].as_array().cloned().unwrap_or_default();
            if downloads.is_empty() {
                return Err(format!("模组文件 {} 缺少下载地址", rel));
            }
            let mut last_err: Option<String> = None;
            for url in downloads {
                let u = url.as_str().unwrap_or("");
                if u.is_empty() {
                    continue;
                }
                match download_mod_file(&client, u, &dest, sha1.as_deref(), None, None).await {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => last_err = Some(e),
                }
            }
            if let Some(e) = last_err {
                return Err(format!("下载模组文件 {} 失败: {}", rel, e));
            }
            if n > 0 {
                let _ = window.emit("download-progress", json!({
                    "stage": format!("下载整合包模组 ({}/{})", i + 1, n),
                    "progress": 100
                }));
            }
        }
        // 3. 解压 overrides
        let _ = window.emit("download-progress", json!({ "stage": "解压整合包覆盖文件", "progress": 100 }));
        extract_zip_dir(&mut archive, "overrides", &version_dir)?;
    } else {
        // CurseForge：批量查询文件下载地址
        let manifest_text = read_pack_file(&mut archive, "manifest.json")
            .ok_or("整合包缺少 manifest.json")?;
        let j: serde_json::Value =
            serde_json::from_str(&manifest_text).map_err(|e| format!("解析整合包清单失败: {}", e))?;
        let file_ids: Vec<serde_json::Value> = j["files"]
            .as_array()
            .map(|a| a.iter().filter_map(|f| f["fileID"].clone().into()).collect())
            .unwrap_or_default();
        if file_ids.is_empty() {
            return Err("整合包文件列表为空".to_string());
        }
        let data = curseforge_request(
            &app_handle,
            &client,
            "mods/files",
            Some(json!({ "fileIds": file_ids })),
        )
        .await
        .map_err(|e| format!("查询整合包文件失败: {}", e))?;
        let files = data["data"].as_array().cloned().unwrap_or_default();
        let n = files.len();
        for (i, f) in files.iter().enumerate() {
            let file_name = f["fileName"].as_str().unwrap_or("").to_string();
            if file_name.is_empty() {
                continue;
            }
            let sha1 = f["hashes"]
                .as_array()
                .and_then(|hs| hs.iter().find(|h| h["algo"].as_u64() == Some(1)))
                .and_then(|h| h["value"].as_str().map(String::from));
            let official = f["downloadUrl"].as_str().unwrap_or("").to_string();
            let mut sources: Vec<String> = Vec::new();
            if official.is_empty() {
                let file_id = f["id"].as_u64().unwrap_or(0);
                let id_str = file_id.to_string();
                let (first4, rest) = if id_str.len() > 4 {
                    (&id_str[..4], id_str[4..].trim_start_matches('0'))
                } else {
                    (id_str.as_str(), "0")
                };
                let rest = if rest.is_empty() { "0" } else { rest };
                let edge = format!("https://edge.forgecdn.net/files/{}/{}/{}", first4, rest, file_name);
                sources.push(edge);
            } else {
                sources.push(official);
            }
            let dest = version_dir.join("mods").join(&file_name);
            let mut last_err: Option<String> = None;
            for src in &sources {
                match download_mod_file(&client, src, &dest, sha1.as_deref(), None, None).await {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => last_err = Some(e),
                }
            }
            if let Some(e) = last_err {
                return Err(format!("下载模组文件 {} 失败: {}", file_name, e));
            }
            if n > 0 {
                let _ = window.emit("download-progress", json!({
                    "stage": format!("下载整合包模组 ({}/{})", i + 1, n),
                    "progress": 100
                }));
            }
        }
        // 3. 解压 overrides（manifest.json 可指定目录名，默认 overrides）
        let override_dir = j["overrides"].as_str().unwrap_or("overrides");
        let _ = window.emit("download-progress", json!({ "stage": "解压整合包覆盖文件", "progress": 100 }));
        extract_zip_dir(&mut archive, override_dir, &version_dir)?;
    }

    // 4. 同名冲突处理：用户指定了自定义版本名时，安装完成后把版本目录改名
    // （rename_version 会校验目标名不存在，含 json/jar/id/inheritsFrom 同步更新）
    let custom = custom_version_name.trim();
    if !custom.is_empty() && custom != info.mc_version {
        dl_log(&format!("整合包安装到自定义版本名: {} → {}", info.mc_version, custom));
        rename_version(app_handle.clone(), info.mc_version.clone(), custom.to_string()).await?;
    }

    let _ = window.emit("download-progress", json!({ "stage": "整合包安装完成", "progress": 100 }));
    let final_name = if custom.is_empty() { info.mc_version.clone() } else { custom.to_string() };
    dl_log(&format!("整合包 {} 安装完成（版本名 {}）", info.name, final_name));
    Ok(final_name)
}

// ========== 3.14 陶瓦联机（Terracotta） ==========
// 集成方式参考 HMCL（HMCL-dev/HMCL 的 terracotta 模块）：
//   1. 运行 terracotta --hmcl <临时文件>，就绪后文件写入 {"port": N}
//   2. HTTP API（http://127.0.0.1:{port}/）：
//      GET /state                          查询状态（host-ok/guest-ok 含房间码）
//      GET /state/scanning?player=..&public_nodes=..   开房
//      GET /state/guesting?room=..&player=..&public_nodes=..  加入
//      GET /log?fetch=true                 日志
//   3. 公共节点列表：https://terracotta.glavo.site/nodes
// 许可：AGPL v3 + 例外（打包未修改二进制 + HTTP API 交互不污染本程序）

const TERRA_INCOTTA_VERSION: &str = "0.4.2";

fn terracotta_classifier() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-x86_64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows-x86_64")
    } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
        Some("windows-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x86_64")
    } else {
        None
    }
}

struct TerracottaProcess {
    child: Option<std::process::Child>,
    port: u32,
}

static TERRACOTTA: std::sync::OnceLock<std::sync::Mutex<TerracottaProcess>> =
    std::sync::OnceLock::new();

fn terracotta_state() -> &'static std::sync::Mutex<TerracottaProcess> {
    TERRACOTTA.get_or_init(|| std::sync::Mutex::new(TerracottaProcess { child: None, port: 0 }))
}

/// 确保 Terracotta 二进制已就位（下载 + 解压），返回其路径
async fn terracotta_ensure_binary(game_dir: &Path) -> Result<PathBuf, String> {
    let classifier = terracotta_classifier().ok_or("当前平台不支持陶瓦联机")?;
    let bin_dir = game_dir.join("terracotta");
    fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;

    let exe_name = if cfg!(windows) { "terracotta.exe" } else { "terracotta" };
    let bin_path = bin_dir.join(exe_name);
    if bin_path.exists() {
        return Ok(bin_path);
    }

    let file_name = format!("terracotta-{}-{}-pkg.tar.gz", TERRA_INCOTTA_VERSION, classifier);
    // 镜像顺序与 HMCL 的 downloads_CN 一致：gitee → cnb.cool → alist.8mi.tech，GitHub 放最后
    // （国内网络直连 GitHub Releases 极慢/超时，会卡住整个下载）
    let download_urls = [
        format!("https://gitee.com/burningtnt/Terracotta/releases/download/v{}/{}", TERRA_INCOTTA_VERSION, file_name),
        format!("https://cnb.cool/HMCL-Terracotta/Terracotta/-/releases/download/v{}/{}", TERRA_INCOTTA_VERSION, file_name),
        format!("https://alist.8mi.tech/d/mirror/HMCL-Terracotta/Auto/v{}/{}", TERRA_INCOTTA_VERSION, file_name),
        format!("https://github.com/burningtnt/Terracotta/releases/download/v{}/{}", TERRA_INCOTTA_VERSION, file_name),
    ];

    let tmp_gz = bin_dir.join(&file_name);
    let client = modrinth_client()?;
    let mut last_err: Option<String> = None;
    for url in &download_urls {
        dl_log(&format!("陶瓦联机组件下载源 {}: {}", download_urls.iter().position(|u| u == url).unwrap_or(0) + 1, url));
        // 单源 240s 总超时：避免某个源卡死整个下载（GitHub 直连经常长时间无响应）
        match download_mod_file(&client, url, &tmp_gz, None, None, Some(240)).await {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                dl_log(&format!("陶瓦联机下载失败 {}: {}", url, e));
                last_err = Some(e);
            }
        }
    }
    if let Some(e) = last_err {
        return Err(format!("陶瓦联机组件下载失败: {}", e));
    }

    // 解压 tar.gz，取出裸二进制（跳过 .pkg 条目）
    // 注意：Windows 包里除 terracotta.exe 外还有 VCRUNTIME140.DLL（VC++ 运行库），
    // 必须一起解压到同目录，否则 exe 缺 DLL 导致 easytier 初始化失败、连不上节点。
    // 参考 Verse（server/terracotta.js）的 ensureTerracottaInstalled：会复制所有 .dll。
    let gz = fs::File::open(&tmp_gz).map_err(|e| e.to_string())?;
    let decoder = flate2::read::GzDecoder::new(gz);
    let mut archive = tar::Archive::new(decoder);
    let mut extracted = false;
    for entry in archive.entries().map_err(|e| format!("解析压缩包失败: {}", e))? {
        let mut entry = entry.map_err(|e| format!("读取压缩包失败: {}", e))?;
        let name = entry.path().map_err(|e| e.to_string())?.to_string_lossy().to_string();
        if name.contains(&format!("terracotta-{}-{}", TERRA_INCOTTA_VERSION, classifier))
            && !name.ends_with(".pkg")
        {
            let mut out = fs::File::create(&bin_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败: {}", e))?;
            extracted = true;
        } else if cfg!(windows) && name.ends_with(".dll") {
            // Windows：把 DLL 复制到二进制同目录（只取文件名，不建子目录）
            let dll_name = Path::new(&name).file_name().unwrap_or_default().to_string_lossy().to_string();
            let dll_path = bin_dir.join(&dll_name);
            let mut out = fs::File::create(&dll_path).map_err(|e| format!("解压 DLL 失败: {}", e))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压 DLL 失败: {}", e))?;
            dl_log(&format!("已解压 DLL: {}", dll_name));
        }
    }
    let _ = fs::remove_file(&tmp_gz);
    if !extracted {
        let _ = fs::remove_file(&bin_path);
        return Err("压缩包中未找到 Terracotta 可执行文件".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755));
    }
    dl_log(&format!("陶瓦联机组件就绪: {}", bin_path.display()));
    Ok(bin_path)
}

/// 启动 Terracotta 守护进程，返回本地 HTTP API 端口
#[tauri::command]
async fn terracotta_start(app_handle: tauri::AppHandle) -> Result<u32, String> {
    {
        let mut guard = terracotta_state().lock().map_err(|e| e.to_string())?;
        if let Some(child) = guard.child.as_mut() {
            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                return Ok(guard.port); // 已在运行
            }
        }
    }

    let game_dir = get_game_dir(&app_handle).await;
    let bin_path = terracotta_ensure_binary(&game_dir).await?;

    #[cfg(target_os = "macos")]
    let port = {
        // macOS 上 --hmcl 会尝试 launchctl 启动一个只有安装 .pkg 才有的 daemon plist，
        // 裸二进制直接跑会 panic。绕法：先自行启动 --daemon，从日志解析端口。
        // 注意：不能用"找最新目录"猜日志——新 daemon 的 application.log 写入有延迟，
        // 期间 ~/terracotta 下最新目录仍是上一次的旧 daemon，会解析到旧端口。
        // 目录名自带 PID 后缀（如 2026-08-16-21-40-09-79354），按 child.pid 精确匹配。
        let log_root = user_home().join("terracotta");

        // 重要：Terracotta 的全局锁（terracotta.lock，2 字节大端端口 + flock）。
        // --daemon 模式必须是唯一实例：如果已有 daemon 在运行（锁被占用），
        // 再启动一个 --daemon 会直接 panic（SIGTRAP 崩溃）。
        // 场景：launcher 重启后本进程的 TERRACOTTA 状态丢失（child=None），
        // 但之前启动的 daemon 进程还活着 → 直接复用其端口，不再重复启动。
        if let Ok(buf) = fs::read(log_root.join("terracotta.lock")) {
            if buf.len() == 2 {
                let existing_port = ((buf[0] as u32) << 8) | buf[1] as u32;
                let probe = reqwest::Client::builder()
                    .timeout(Duration::from_secs(2))
                    .build()
                    .map_err(|e| e.to_string())?;
                if let Ok(resp) = probe
                    .get(format!("http://127.0.0.1:{}/state", existing_port))
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        // 复用时先检查状态：若残留的是 exception（如上次开房失败 type=4），
                        // 直接复用会让前端"点启动就看到状态4"。先重置到 waiting。
                        let mut need_reset = true;
                        if let Ok(text) = resp.text().await {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                if v["state"].as_str() == Some("waiting") {
                                    need_reset = false;
                                }
                            }
                        }
                        if need_reset {
                            dl_log(&format!("复用的陶瓦守护进程非 waiting 状态，重置到 waiting"));
                            let reset_client = reqwest::Client::builder()
                                .timeout(Duration::from_secs(3))
                                .build()
                                .map_err(|e| e.to_string())?;
                            let _ = reset_client
                                .get(format!("http://127.0.0.1:{}/state/ide", existing_port))
                                .send()
                                .await;
                        }
                        dl_log(&format!("探测到已有陶瓦联机守护进程（端口 {}），直接复用", existing_port));
                        {
                            let mut guard = terracotta_state().lock().map_err(|e| e.to_string())?;
                            guard.port = existing_port;
                        }
                        return Ok(existing_port);
                    }
                }
                dl_log(&format!("锁文件存在但端口 {} 无响应，将重新启动", existing_port));
            }
        }

        let mut child = std::process::Command::new(&bin_path)
            .arg("--daemon")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("启动陶瓦联机失败: {}", e))?;
        let child_pid = child.id().to_string();

        let mut port: u32 = 0;
        for _ in 0..300 {
            // 只认本次 spawn 的 daemon 的日志目录（名称以 PID 结尾）
            let found = terracotta_log_dirs().into_iter().find(|(name, _)| name.ends_with(&child_pid));
            if let Some((_, log)) = found {
                if let Ok(content) = fs::read_to_string(&log) {
                    let marker = "Rocket has launched from http://127.0.0.1:";
                    if let Some(idx) = content.find(marker) {
                        let rest = &content[idx + marker.len()..];
                        if let Some(num) = rest.split(|c: char| !c.is_ascii_digit()).next() {
                            if let Ok(p) = num.parse::<u32>() {
                                port = p;
                                break;
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if port == 0 {
            let _ = child.kill();
            return Err("陶瓦联机启动超时，请检查网络后重试".to_string());
        }
        // 端口已从日志解析，但 Rocket 可能还没开始接受连接；
        // 轮询 /state 直到可访问，避免前端立刻请求时报"请求失败"。
        let ready_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| e.to_string())?;
        let mut ready = false;
        for _ in 0..50 {
            if let Ok(r) = ready_client.get(format!("http://127.0.0.1:{}/state", port)).send().await {
                if r.status().is_success() {
                    ready = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if !ready {
            let _ = child.kill();
            return Err("陶瓦联机启动后无响应，请重试".to_string());
        }
        {
            let mut guard = terracotta_state().lock().map_err(|e| e.to_string())?;
            guard.child = Some(child);
            guard.port = port;
        }
        port
    };

    #[cfg(not(target_os = "macos"))]
    let port = {
        // Windows / Linux：--hmcl 端口文件方式（与 HMCL 一致）
        // 重要：Terracotta 的 --hmcl 检测到已有 daemon（锁文件 Secondary）会直接复用旧进程。
        // 若上次残留的是异常状态（如 type=4），点"启动"会复用坏状态 → 前端立即显示状态4。
        // 参考 Verse（server/terracotta.js killExistingTerracotta）：每次启动前先杀掉残留进程，
        // 保证是全新 waiting 状态的 daemon。
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/IM", "terracotta.exe"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("pkill")
                .args(["-f", "terracotta"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        let port_file = std::env::temp_dir().join(format!("lumia-terracotta-{}.port", std::process::id()));
        let _ = fs::remove_file(&port_file);

        let mut child = std::process::Command::new(&bin_path)
            .arg("--hmcl")
            .arg(&port_file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("启动陶瓦联机失败: {}", e))?;

        let mut port: u32 = 0;
        for _ in 0..200 {
            if port_file.exists() {
                if let Ok(s) = fs::read_to_string(&port_file) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                        port = v["port"].as_u64().unwrap_or(0) as u32;
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if port == 0 {
            let _ = child.kill();
            return Err("陶瓦联机启动超时，请检查网络后重试".to_string());
        }
        // 就绪检查：等 Rocket 开始接受连接，避免前端立即请求失败
        let ready_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| e.to_string())?;
        let mut ready = false;
        for _ in 0..50 {
            if let Ok(r) = ready_client.get(format!("http://127.0.0.1:{}/state", port)).send().await {
                if r.status().is_success() {
                    ready = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if !ready {
            let _ = child.kill();
            return Err("陶瓦联机启动后无响应，请重试".to_string());
        }
        {
            let mut guard = terracotta_state().lock().map_err(|e| e.to_string())?;
            guard.child = Some(child);
            guard.port = port;
        }
        port
    };

    dl_log(&format!("陶瓦联机已启动，端口 {}", port));
    Ok(port)
}

/// 请求 Terracotta 本地 API
/// 注意：/state/scanning 和 /state/guesting 会立即返回 200 但响应体为空，
/// 实际的扫描/加入在后台进行，结果要通过轮询 /state 获取。
async fn terracotta_api(port: u32, path_and_query: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("http://127.0.0.1:{}/{}", port, path_and_query);
    let resp = client.get(&url).send().await.map_err(|e| format!("陶瓦联机请求失败: {}", e))?;
    if !resp.status().is_success() {
        // 陶瓦用 Rocket 默认 400 表示"房间码无效"或"当前状态不允许该操作"（如非 waiting 状态加入）
        if resp.status() == reqwest::StatusCode::BAD_REQUEST {
            if path_and_query.starts_with("state/guesting") {
                return Err("房间码无效或当前状态无法加入（需先断开/退出当前房间）".to_string());
            }
            return Err("操作失败：当前状态不允许该操作（需先回到待机状态）".to_string());
        }
        return Err(format!("陶瓦联机 API HTTP {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {}", e))?;
    if text.trim().is_empty() {
        // 空响应体：请求已受理，后台进行中，由轮询 /state 获得结果
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {}", e))
}
/// 查询联机状态（host-ok / guest-ok 时含房间码）
/// 注意：easytier 建隧道失败进入 exception 时，/state 响应不带 code 字段，
/// 但房间码在日志里已经生成过（HostStarting/HostOk 行）。这里从日志补上，
/// 否则前端永远拿不到房间码（HMCL 同样是靠 /state 的 code 展示）。
#[tauri::command]
async fn terracotta_status(port: u32) -> Result<serde_json::Value, String> {
    let mut state = terracotta_api(port, "state").await?;
    if let Some(obj) = state.as_object_mut() {
        let has_code = obj
            .get("code")
            .and_then(|c| c.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if !has_code {
            if let Some(code) = terracotta_last_room_code() {
                obj.insert("code".to_string(), serde_json::Value::String(code));
            }
        }
    }
    Ok(state)
}

/// 用户主目录（跨平台）：Windows 用 USERPROFILE，macOS/Linux 用 HOME。
/// 注意：Windows 没有 HOME 环境变量，直接用 HOME 会得到空串 → 找不到陶瓦日志目录。
fn user_home() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("USERPROFILE") {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// 列出 ~/terracotta 下所有含 application.log 的日志目录，按目录修改时间从新到旧排序。
/// 返回 (目录名, application.log 路径)。无日志目录时返回空列表。
fn terracotta_log_dirs() -> Vec<(String, PathBuf)> {
    let log_root = user_home().join("terracotta");
    let mut dirs: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
    if let Ok(rd) = fs::read_dir(&log_root) {
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let log = e.path().join("application.log");
            if !log.exists() {
                continue;
            }
            let mt = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            dirs.push((mt, e.file_name().to_string_lossy().to_string(), log));
        }
    }
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.into_iter().map(|(_, name, log)| (name, log)).collect()
}

/// 从最近的 Terracotta 日志解析最近一次生成的房间码
/// （日志行格式：`[State]: Switch to AppState::HostStarting { code: "U/XXXX-...", port: N }`）
fn terracotta_last_room_code() -> Option<String> {
    // 最新的日志目录优先，找到第一个含房间码的
    for (_, log) in terracotta_log_dirs() {
        let Ok(content) = fs::read_to_string(&log) else { continue };
        let mut last: Option<String> = None;
        for line in content.lines() {
            if let Some(idx) = line.find("code: \"") {
                let rest = &line[idx + 7..];
                if let Some(end) = rest.find('"') {
                    last = Some(rest[..end].to_string());
                }
            }
        }
        if let Some(code) = last {
            return Some(code);
        }
    }
    None
}

/// 拉取公共节点列表
async fn terracotta_nodes() -> Vec<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let mut nodes = Vec::new();
    if let Ok(resp) = client.get("https://terracotta.glavo.site/nodes").send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(arr) = json.as_array() {
                for n in arr {
                    if let Some(url) = n["url"].as_str() {
                        nodes.push(url.to_string());
                    }
                }
            }
        }
    }
    nodes
}

/// 检查最近一次陶瓦日志中，该节点的 easytier 连接记录是否为错误。
/// TCP 能连上不代表 easytier 握手成功（如节点后端已死但网关仍接受连接），
/// 用真实日志纠正：日志里**最近出现**该节点的行若是 error / reset / timeout → 判定不可用。
/// 注意：判定必须基于"最近出现该节点的行"本身，不能跳过它去找更早的含关键词的行，
/// 否则历史上任何一次抖动（一次 Connection reset）都会把节点永久判死，出现"所有官方节点全挂"的假象。
fn terracotta_log_node_failed(url: &str) -> bool {
    // 只看最新的日志目录
    let Some((_, log)) = terracotta_log_dirs().into_iter().next() else {
        return false;
    };
    let Ok(content) = fs::read_to_string(&log) else { return false };
    for line in content.lines().rev() {
        if !line.contains(url) {
            continue;
        }
        // 最近出现的行：是错误行 → 失败；不是（成功/心跳/其他）→ 不判失败
        return line.contains("error") || line.contains("Connection reset") || line.contains("timeout");
    }
    false
}

/// 探测单个公共节点是否可达：
/// - https:// 地址：先 GET 取到重定向后的 tcp://host:port（与陶瓦 easytier 相同的方式），再 TCP 探测
/// - tcp://host:port 等直连地址：直接 TCP 探测
/// 返回 (最终探测地址, 状态)。状态：
/// - Ok(Some(true))  可达
/// - Ok(Some(false)) 明确不可达（TCP 都连不上 / 日志判定握手失败）
/// - Ok(None)        未知（网关 GET 失败拿不到 tcp 地址——网络抖动/被墙常见，不能当成"挂了"）
async fn terracotta_probe_node(url: &str) -> (String, Option<bool>) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default();
    let addr = if url.starts_with("http://") || url.starts_with("https://") {
        match client.get(url).send().await {
            Ok(resp) => match resp.text().await {
                Ok(body) => body.trim().to_string(),
                Err(_) => url.to_string(),
            },
            Err(_) => url.to_string(),
        }
    } else {
        url.to_string()
    };
    let host_port = addr
        .strip_prefix("tcp://")
        .or_else(|| addr.strip_prefix("udp://"))
        .or_else(|| addr.strip_prefix("ws://"))
        .unwrap_or("");
    if host_port.is_empty() {
        // 拿不到 tcp 地址：可能是网关 GET 失败（国内访问海外域名经常超时/被墙），
        // 不代表节点真的挂了——标"未知"，让前端显示为待确认而非红点。
        return (addr, None);
    }
    let mut parts = host_port.rsplitn(2, ':');
    let port = parts.next().and_then(|p| p.parse::<u16>().ok());
    let host = parts.next().unwrap_or("");
    let (Some(port), false) = (port, host.is_empty()) else {
        return (addr, None);
    };
    let tcp_ok = tokio::time::timeout(
        Duration::from_secs(8),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false);
    if !tcp_ok {
        return (addr, Some(false));
    }
    // TCP 通，但最近日志里该节点握手一直失败 → 视为不可用
    if terracotta_log_node_failed(url) {
        return (addr, Some(false));
    }
    (addr, Some(true))
}

/// 公共节点可达性检查（供前端展示"节点状态"，也用于判断开房/加入前是否值得提示）
#[tauri::command]
async fn terracotta_nodes_status(custom_nodes: Option<Vec<String>>) -> Result<Vec<serde_json::Value>, String> {
    let mut nodes = terracotta_nodes().await;
    if let Some(custom) = custom_nodes {
        for c in custom {
            let c = c.trim().to_string();
            if !c.is_empty() && !nodes.contains(&c) {
                nodes.push(c);
            }
        }
    }
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        let (target, ok) = terracotta_probe_node(&n).await;
        // ok: true 可达 / false 明确不可达 / null 未知（网关 GET 失败，不能当挂）
        out.push(json!({ "url": n, "target": target, "ok": ok }));
    }
    Ok(out)
}

fn terracotta_query(player: &str, room: Option<&str>, nodes: &[String]) -> String {
    let mut q = format!("player={}", url_encode(player));
    if let Some(room) = room {
        q.push_str(&format!("&room={}", url_encode(room)));
    }
    for n in nodes {
        q.push_str(&format!("&public_nodes={}", url_encode(n)));
    }
    q
}

/// 组装传给陶瓦的公共节点列表：自定义节点在前（用户自己的共享节点优先被 easytier 尝试），
/// 官方拉取到的公共节点在后。陶瓦自身还会追加它内置的节点，无法去除。
async fn terracotta_merge_nodes(custom_nodes: Option<Vec<String>>) -> Vec<String> {
    let mut nodes: Vec<String> = custom_nodes
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    nodes.extend(terracotta_nodes().await);
    nodes
}

/// 开房：扫描并开始联机，成功后 /state 返回房间码
#[tauri::command]
async fn terracotta_host(port: u32, player_name: String, custom_nodes: Option<Vec<String>>) -> Result<serde_json::Value, String> {
    // 陶瓦只允许 waiting 状态开房；若残留旧状态（host-scanning/host-ok/exception），
    // /state/scanning 会被静默忽略（返回 200 但什么都不做）→ 前端看到旧房间码"像创了个房间"。
    // 参考 Verse/HMCL：每次操作前先 /state/ide 重置到 waiting。
    terracotta_reset_if_busy(port).await?;
    let nodes = terracotta_merge_nodes(custom_nodes).await;
    let q = terracotta_query(&player_name, None, &nodes);
    terracotta_api(port, &format!("state/scanning?{}", q)).await
}

/// 加入房间
#[tauri::command]
async fn terracotta_join(port: u32, room: String, player_name: String, custom_nodes: Option<Vec<String>>) -> Result<serde_json::Value, String> {
    // 同上：guesting 只接受 waiting 状态，非 waiting 直接 400。
    terracotta_reset_if_busy(port).await?;
    let nodes = terracotta_merge_nodes(custom_nodes).await;
    let q = terracotta_query(&player_name, Some(&room), &nodes);
    terracotta_api(port, &format!("state/guesting?{}", q)).await
}

/// 若陶瓦不在 waiting 状态，先重置（GET /state/ide，与 HMCL setWaiting / Verse 重启进程等价）
async fn terracotta_reset_if_busy(port: u32) -> Result<(), String> {
    if let Ok(state) = terracotta_api(port, "state").await {
        let s = state.get("state").and_then(|v| v.as_str()).unwrap_or("");
        if s != "waiting" {
            dl_log(&format!("陶瓦状态为 {}，先重置到 waiting", s));
            let _ = terracotta_api(port, "state/ide").await;
        }
    }
    Ok(())
}

/// 游戏是否在运行（用于陶瓦联机开房前提示，参考 HMCL）
#[tauri::command]
fn is_game_running() -> bool {
    GAME_RUNNING.load(std::sync::atomic::Ordering::SeqCst)
}

/// 停止扫描/退出房间，回到等待状态（参考 HMCL 的 setWaiting：GET /state/ide）
#[tauri::command]
async fn terracotta_back(port: u32) -> Result<serde_json::Value, String> {
    terracotta_api(port, "state/ide").await
}

/// 停止陶瓦联机
#[tauri::command]
async fn terracotta_stop() -> Result<(), String> {
    let mut guard = terracotta_state().lock().map_err(|e| e.to_string())?;
    let port = guard.port;
    if let Some(mut child) = guard.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    } else if port != 0 {
        // 复用的 daemon（launcher 重启后接管）：没有 child 句柄，
        // 通过日志目录（目录名以 PID 结尾）找到进程并终止
        for (name, log) in terracotta_log_dirs() {
            let Ok(content) = fs::read_to_string(&log) else { continue };
            if !content.contains(&format!("127.0.0.1:{}", port)) {
                continue;
            }
            // 目录名如 2026-08-16-21-51-48-81050，最后一段是 PID
            if let Some(pid_str) = name.rsplit('-').next() {
                if let Ok(pid) = pid_str.parse::<i32>() {
                    #[cfg(unix)]
                    {
                        let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
                    }
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("taskkill")
                            .args(["/PID", &pid.to_string(), "/F"])
                            .status();
                    }
                    dl_log(&format!("已终止复用的陶瓦联机守护进程 PID {}", pid));
                    break;
                }
            }
        }
    }
    guard.port = 0;
    Ok(())
}
// ========== 3.15 AI 助手（Minecraft Wiki 接地 + 崩溃分析 + 文件操作审批） ==========
const WIKI_API: &str = "https://zh.minecraft.wiki/api.php";

#[derive(serde::Serialize, Deserialize, Clone)]
struct AiChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiChatResult {
    reply: String,
    /// 是否注入了 Wiki 参考资料（只有此时前端才需要标注来源）
    used_wiki: bool,
    /// 本次请求的 token 用量（来自 API 响应的 usage 字段）
    usage: AiUsage,
    /// 模型提议的文件操作（需用户审批后才执行）
    tool_calls: Vec<AiToolCall>,
}

#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct AiUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AiToolCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

/// 只读工具：列出游戏目录内的目录/文件（供 AI 自行查找日志）
fn exec_list_dir(base: &Path, args: &serde_json::Value) -> Result<String, String> {
    let path = args["path"].as_str().unwrap_or("").to_string();
    let dir = if path.is_empty() {
        base.to_path_buf()
    } else {
        resolve_game_path(base, &path)?
    };
    if !dir.is_dir() {
        return Err(format!("不是目录: {}", path));
    }
    let mut out = String::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for (i, e) in rd.flatten().enumerate() {
            if i >= 200 {
                out.push_str("…(条目过多，已截断)\n");
                break;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let kind = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) { "DIR " } else { "FILE" };
            out.push_str(&format!("{} {}\n", kind, name));
        }
    }
    if out.is_empty() {
        out.push_str("(空目录)\n");
    }
    Ok(out)
}

/// 只读工具：读取游戏目录内的文本文件（截断 + 跳过二进制）
fn exec_read_file(base: &Path, args: &serde_json::Value) -> Result<String, String> {
    let path = args["path"].as_str().unwrap_or("").to_string();
    let max = args["max_chars"].as_u64().unwrap_or(20000) as usize;
    let target = resolve_game_path(base, &path)?;
    if !target.is_file() {
        return Err(format!("文件不存在: {}", path));
    }
    let bytes = fs::read(&target).map_err(|e| format!("读取失败 {}: {}", path, e))?;
    if bytes.contains(&0) {
        return Ok("(二进制文件，跳过读取)".to_string());
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.chars().take(max).collect::<String>())
}

/// 联网搜索：仅查询中文 Minecraft Wiki（MediaWiki API），返回最多 5 条「标题 / 摘要 / 链接」纯文本摘要。
/// 供无内置搜索的模型调用，保证知识来源统一（国内可直连、无需 API Key）。
async fn exec_web_search(client: &reqwest::Client, query: &str) -> Result<String, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok("（搜索关键词为空）".to_string());
    }
    // 1. 搜索标题
    let search_url = format!(
        "{}?action=query&format=json&list=search&srsearch={}&srlimit=5&srprop=snippet&formatversion=2",
        WIKI_API,
        url_encode(q)
    );
    let search_json: serde_json::Value = client
        .get(&search_url)
        .header("User-Agent", "LumiaLauncher/0.1 (Minecraft launcher; wiki assistant)")
        .send()
        .await
        .map_err(|e| format!("Wiki 搜索请求失败: {}", e))?
        .json()
        .await
        .map_err(|e| format!("解析 Wiki 搜索响应失败: {}", e))?;

    let results = search_json["query"]["search"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if results.is_empty() {
        return Ok("（未在 Minecraft Wiki 中找到相关结果）".to_string());
    }
    let titles: Vec<String> = results
        .iter()
        .filter_map(|r| r["title"].as_str().map(String::from))
        .collect();

    // 2. 取简介摘要 + 完整链接
    let extract_url = format!(
        "{}?action=query&format=json&prop=extracts|info&exintro&explaintext&inprop=url&titles={}&formatversion=2",
        WIKI_API,
        url_encode(&titles.join("|"))
    );
    let pages_json: serde_json::Value = client
        .get(&extract_url)
        .header("User-Agent", "LumiaLauncher/0.1 (Minecraft launcher; wiki assistant)")
        .send()
        .await
        .map_err(|e| format!("Wiki 摘要请求失败: {}", e))?
        .json()
        .await
        .map_err(|e| format!("解析 Wiki 摘要响应失败: {}", e))?;

    let pages = pages_json["query"]["pages"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut items: Vec<String> = Vec::new();
    for page in pages {
        let title = page["title"].as_str().unwrap_or("").to_string();
        let extract = page["extract"].as_str().unwrap_or("").trim().to_string();
        let fullurl = page["fullurl"].as_str().unwrap_or("").to_string();
        if title.is_empty() {
            continue;
        }
        let link = if !fullurl.is_empty() {
            fullurl
        } else {
            format!("https://zh.minecraft.wiki/{}", url_encode(&title.replace(' ', "_")))
        };
        let mut line = title;
        if !extract.is_empty() {
            line.push_str(&format!("\n  {}", truncate_str(&extract, 300)));
        }
        line.push_str(&format!("\n  {}", link));
        items.push(line);
        if items.len() >= 5 {
            break;
        }
    }
    if items.is_empty() {
        return Ok("（未在 Minecraft Wiki 中找到相关结果）".to_string());
    }
    Ok(items.join("\n\n"))
}

/// 查询中文 Minecraft Wiki（MediaWiki API），返回前几条结果摘要作为模型上下文
async fn ai_wiki_context(query: &str) -> String {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("LumiaLauncher/0.1 (Minecraft launcher; wiki assistant)")
        .build()
        .unwrap_or_default();
    // 1. 搜索标题
    let search_url = format!(
        "{}?action=query&format=json&list=search&srsearch={}&srlimit=3&formatversion=2",
        WIKI_API,
        url_encode(query)
    );
    let titles: Vec<String> = match client.get(&search_url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => v["query"]["search"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s["title"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => vec![],
        },
        Err(_) => vec![],
    };
    if titles.is_empty() {
        return String::new();
    }
    // 2. 取简介摘要
    let extract_url = format!(
        "{}?action=query&format=json&prop=extracts&exintro&explaintext&titles={}&formatversion=2",
        WIKI_API,
        url_encode(&titles.join("|"))
    );
    let mut out = String::new();
    if let Ok(resp) = client.get(&extract_url).send().await {
        if let Ok(v) = resp.json::<serde_json::Value>().await {
            if let Some(pages) = v["query"]["pages"].as_array() {
                for page in pages {
                    if let (Some(title), Some(extract)) = (page["title"].as_str(), page["extract"].as_str()) {
                        if !extract.trim().is_empty() {
                            out.push_str(&format!("【{}】{}\n", title, extract));
                        }
                    }
                }
            }
        }
    }
    out
}

/// AI 系统提示词：Minecraft 助手 + Wiki 来源标注（仅用了 Wiki 资料时）+ 文件操作只提议不执行
fn ai_system_prompt() -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    format!(
        "你是 Lumia Launcher 内置的 Minecraft 助手，知识主要来源于中文 Minecraft Wiki（https://zh.minecraft.wiki）。\n\
         今天是 {today}。你的训练数据可能过时，凡涉及「最新版本、版本号、新增内容、更新日志、最新模组」等时效性信息，必须先调用 web_search 工具联网搜索中文 Minecraft Wiki，再依据搜索结果回答，不要仅凭旧知识作答。\n\
         规则：\n\
         1. 回答 Minecraft 相关问题，优先依据用户问题对应的 Wiki 参考资料或 web_search 搜索结果（若提供）。\n\
         2. 只有当你实际使用了 Wiki 参考资料或 web_search 结果来回答时，才在回答结尾另起一行标注：本回答内容来源于 Minecraft Wiki（CC BY-NC-SA 3.0）；未使用时不要添加该标注。\n\
         3. 排查问题时你可以使用 list_dir 和 read_file 工具查看游戏目录里的文件（path 用相对游戏目录的路径，如 logs 或 versions/xxx/mods）。读取会自动执行，无需用户批准。常见日志位置：crash-reports/（崩溃报告 .txt）与 logs/latest.log（游戏运行日志）。\n\
         4. 用户请求分析崩溃日志时，先用工具自行找到并读取相关日志，再结合日志给出可能原因与解决建议（如模组冲突、Java 版本、内存不足等）。\n\
         5. 如果修复需要修改文件（删除损坏文件、写入配置文件等），只调用 modify_files 工具提议，路径必须是相对游戏目录的路径。绝不直接执行任何文件操作，等待用户批准。\n\
         6. 涉及最新信息（新版模组、报错解决方案、版本更新、版本号等）时，先用 web_search 工具联网搜索，再依据结果回答。"
    )
}

/// 单次调用 OpenAI 兼容接口，返回（回复, 工具调用, 用量）
/// 流式调用 OpenAI 兼容接口（SSE），通过 tauri 事件推送内容/推理/状态。
/// 分块空闲 60s 超时（不再整段 90s 超时导致中途中断）。
async fn ai_call_api_streaming(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    model: &str,
    full: &[serde_json::Value],
    tools: &serde_json::Value,
    app_handle: &tauri::AppHandle,
) -> Result<(String, Vec<AiToolCall>, AiUsage), String> {
    let body = serde_json::json!({
        "model": model,
        "messages": full,
        "tools": tools,
        "tool_choice": "auto",
        "temperature": 0.3,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求 AI 服务失败（请检查网络与 API 地址）: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("AI 服务返回错误（HTTP {}）: {}", status, truncate_str(&text, 300)));
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut usage = AiUsage::default();
    let mut tool_calls: std::collections::BTreeMap<usize, AiToolCall> = std::collections::BTreeMap::new();
    let mut done = false;

    while !done {
        // 分块空闲超时：60s 无新数据视为异常
        let chunk = tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .map_err(|_| "AI 响应超时（长时间无输出）".to_string())?;
        let chunk = match chunk {
            Some(Ok(c)) => c,
            Some(Err(e)) => return Err(format!("读取 AI 响应失败: {}", e)),
            None => break,
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();
            let line = line.trim();
            if line.is_empty() || !line.starts_with("data:") {
                continue;
            }
            let data = line[5..].trim();
            if data == "[DONE]" {
                done = true;
                break;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            if v["usage"].is_object() {
                usage.prompt_tokens = v["usage"]["prompt_tokens"].as_u64().unwrap_or(usage.prompt_tokens);
                usage.completion_tokens = v["usage"]["completion_tokens"].as_u64().unwrap_or(usage.completion_tokens);
                usage.total_tokens = v["usage"]["total_tokens"].as_u64().unwrap_or(usage.total_tokens);
            }
            let Some(choice) = v["choices"].as_array().and_then(|a| a.first()) else { continue };
            let delta = &choice["delta"];
            if let Some(c) = delta["content"].as_str() {
                if !c.is_empty() {
                    content.push_str(c);
                    let _ = app_handle.emit("ai-stream", json!({ "kind": "content", "text": c }));
                }
            }
            if let Some(r) = delta["reasoning_content"].as_str() {
                if !r.is_empty() {
                    reasoning.push_str(r);
                    let _ = app_handle.emit("ai-stream", json!({ "kind": "reasoning", "text": r }));
                }
            }
            if let Some(calls) = delta["tool_calls"].as_array() {
                for c in calls {
                    let idx = c["index"].as_u64().unwrap_or(0) as usize;
                    let id = c["id"].as_str().unwrap_or("");
                    let name = c["function"]["name"].as_str().unwrap_or("");
                    let args = c["function"]["arguments"].as_str().unwrap_or("");
                    let entry = tool_calls.entry(idx).or_insert_with(|| AiToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: serde_json::Value::Null,
                    });
                    if !id.is_empty() {
                        entry.id = id.to_string();
                    }
                    if !name.is_empty() {
                        entry.name = name.to_string();
                    }
                    if !args.is_empty() {
                        let current = match &entry.arguments {
                            serde_json::Value::String(s) => s.clone(),
                            _ => String::new(),
                        };
                        entry.arguments = serde_json::Value::String(current + args);
                    }
                }
            }
        }
    }
    let mut out_calls = Vec::new();
    for (_, mut c) in tool_calls {
        if let serde_json::Value::String(s) = &c.arguments {
            c.arguments = serde_json::from_str(s).unwrap_or(serde_json::json!({}));
        }
        out_calls.push(c);
    }
    Ok((content, out_calls, usage))
}

/// 聊天：注入 wiki 上下文 → 调用 OpenAI 兼容接口（最多 6 轮）→ 返回回复与需审批的工具提议。
/// 只读工具（list_dir / read_file）在循环内自动执行；modify_files 只收集、不执行，交给前端审批。
#[tauri::command]
async fn ai_chat(messages: Vec<AiChatMessage>, app_handle: tauri::AppHandle) -> Result<AiChatResult, String> {
    let state = app_handle.state::<AppState>();
    let (enabled, base_url, api_key, model, provider) = {
        let cfg = state.config.lock().await;
        (
            cfg.ai_enabled,
            cfg.ai_base_url.clone(),
            cfg.ai_api_key.clone(),
            cfg.ai_model.clone(),
            cfg.ai_provider.clone(),
        )
    };
    if !enabled {
        return Err("AI 助手未开启，请先在设置中配置并启用".to_string());
    }
    if api_key.is_empty() {
        return Err("未配置 AI API Key，请先在设置中填写".to_string());
    }

    // 组装消息：系统提示 + （对短问题）注入 wiki 参考资料 + 对话历史
    let mut full: Vec<serde_json::Value> = Vec::new();
    full.push(serde_json::json!({ "role": "system", "content": ai_system_prompt() }));

    let mut used_wiki = false;
    let mut last_user: Option<String> = None;
    for m in &messages {
        if m.role == "user" {
            last_user = Some(m.content.clone());
        }
    }
    // 只有"问题类"短消息才查 wiki（崩溃日志等长文本不查）
    if let Some(q) = last_user {
        if q.chars().count() < 300 {
            let ctx = ai_wiki_context(&q).await;
            if !ctx.is_empty() {
                used_wiki = true;
                full.push(serde_json::json!({
                    "role": "system",
                    "content": format!("以下是用户问题相关的 Minecraft Wiki 参考资料，请优先依据它们回答：\n{}", ctx)
                }));
            }
        }
    }
    for m in &messages {
        full.push(serde_json::json!({ "role": m.role, "content": m.content }));
    }

    // 工具定义：只读工具自动执行；modify_files 需用户审批
    let tools = serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "list_dir",
                "description": "列出游戏目录内某个目录的内容（相对路径，如 logs 或 versions）。用于自行查找日志/文件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "相对游戏目录的目录路径，空表示根目录" }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "读取游戏目录内的文本文件内容（相对路径）。自动执行，无需用户批准。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "相对游戏目录的文件路径" },
                        "max_chars": { "type": "integer", "description": "最多读取字符数，默认 20000" }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "modify_files",
                "description": "提议对游戏目录内的文件进行修改（删除或写入）。仅提议，绝不执行，等待用户批准。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "operations": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "action": { "type": "string", "enum": ["delete_file", "write_file"] },
                                    "path": { "type": "string", "description": "相对游戏目录的路径" },
                                    "content": { "type": "string", "description": "write_file 时要写入的内容" }
                                },
                                "required": ["action", "path"]
                            }
                        }
                    },
                    "required": ["operations"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "联网搜索，返回相关网页的标题、摘要与链接。用于获取模型本身不知道的信息（如最新资讯、报错解决方案）。自动执行，无需用户批准。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "搜索关键词" }
                    },
                    "required": ["query"]
                }
            }
        }
    ]);

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    // 流式请求：总超时放宽到 300s（分块空闲 60s 在 ai_call_api_streaming 内处理）
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("创建请求失败: {}", e))?;
    let game_dir = get_game_dir(&app_handle).await;
    let base = game_dir.canonicalize().unwrap_or(game_dir);

    // Anthropic 用原生 Messages API（认证/请求格式均不同），单独走一条路径
    if ai_is_anthropic(&provider, &base_url) {
        return ai_chat_anthropic(&client, &base_url, &api_key, &model, &full, used_wiki, &app_handle, &base).await;
    }

    // 代理循环：执行只读工具；modify_files 停止循环交给前端
    let mut reply = String::new();
    let mut usage_total = AiUsage::default();
    let mut tool_calls_out: Vec<AiToolCall> = Vec::new();
    for _turn in 0..6 {
        let (r, calls, usage) = ai_call_api_streaming(&client, &url, &api_key, &model, &full, &tools, &app_handle).await?;
        usage_total.prompt_tokens += usage.prompt_tokens;
        usage_total.completion_tokens += usage.completion_tokens;
        usage_total.total_tokens += usage.total_tokens;
        reply.push_str(&r);
        if calls.is_empty() {
            break;
        }
        // 工具调用必须有非空 id（流式解析异常时给出明确错误，避免下一轮被 API 拒绝造成"中断"）
        if calls.iter().any(|c| c.id.is_empty()) {
            return Err("AI 工具调用数据异常，请重试".to_string());
        }
        // 出现 modify_files → 收集给前端审批并停止循环
        let modify_calls: Vec<&AiToolCall> = calls.iter().filter(|c| c.name == "modify_files").collect();
        if !modify_calls.is_empty() {
            tool_calls_out.extend(modify_calls.into_iter().cloned());
            break;
        }
        // 把带 tool_calls 的 assistant 消息加入历史
        let calls_json: Vec<serde_json::Value> = calls
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": c.name,
                        "arguments": serde_json::to_string(&c.arguments).unwrap_or_else(|_| "{}".to_string())
                    }
                })
            })
            .collect();
        let mut assistant_msg = serde_json::json!({ "role": "assistant", "content": reply });
        assistant_msg["tool_calls"] = serde_json::Value::Array(calls_json);
        full.push(assistant_msg);
        // 执行只读工具（先通知前端正在做什么）
        for c in &calls {
            let arg = if c.name == "web_search" {
                c.arguments["query"].as_str().unwrap_or("").to_string()
            } else {
                c.arguments["path"].as_str().unwrap_or("").to_string()
            };
            let _ = app_handle.emit("ai-stream", json!({ "kind": "status", "tool": c.name, "path": arg }));
            let result = match c.name.as_str() {
                "list_dir" => exec_list_dir(&base, &c.arguments),
                "read_file" => exec_read_file(&base, &c.arguments),
                "web_search" => exec_web_search(&client, c.arguments["query"].as_str().unwrap_or("")).await,
                other => Err(format!("未知工具: {}", other)),
            };
            let content = match result {
                Ok(v) => v,
                Err(e) => format!("错误: {}", e),
            };
            eprintln!("[ai-tool] {} {:?} -> {}", c.name, c.arguments, truncate_str(&content, 300));
            full.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": c.id,
                "content": content
            }));
        }
    }
    Ok(AiChatResult { reply, used_wiki, usage: usage_total, tool_calls: tool_calls_out })
}

fn truncate_str(s: &str, max: usize) -> String {
    let count = s.chars().count();
    let t = s.chars().take(max).collect::<String>();
    if count > max {
        format!("{}…", t)
    } else {
        t
    }
}

#[derive(serde::Deserialize)]
struct AiFileOp {
    action: String,
    path: String,
    #[serde(default)]
    content: Option<String>,
}

/// 解析游戏目录内的目标路径（防穿越）：父目录必须已存在且在 base 内
fn resolve_game_path(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let abs = base.join(rel);
    let parent = abs.parent().ok_or_else(|| format!("非法路径: {}", rel))?;
    let parent_canon = parent
        .canonicalize()
        .map_err(|_| format!("目录不存在: {}", parent.display()))?;
    if !parent_canon.starts_with(&base) {
        return Err(format!("路径越界，已拒绝: {}", rel));
    }
    let file_name = abs.file_name().ok_or_else(|| format!("非法路径: {}", rel))?;
    Ok(parent_canon.join(file_name))
}

/// 执行单个已批准的文件操作（独立函数，便于单测）
fn apply_ai_file_op(base: &Path, op: &AiFileOp) -> Result<serde_json::Value, String> {
    let target = resolve_game_path(base, &op.path)?;
    match op.action.as_str() {
        "delete_file" => {
            if target.is_file() {
                fs::remove_file(&target).map_err(|e| format!("删除失败 {}: {}", op.path, e))?;
                Ok(serde_json::json!({ "path": op.path, "action": "delete_file", "ok": true }))
            } else {
                Ok(serde_json::json!({ "path": op.path, "action": "delete_file", "ok": false, "error": "文件不存在" }))
            }
        }
        "write_file" => {
            let content = op.content.clone().unwrap_or_default();
            fs::write(&target, content).map_err(|e| format!("写入失败 {}: {}", op.path, e))?;
            Ok(serde_json::json!({ "path": op.path, "action": "write_file", "ok": true }))
        }
        other => Err(format!("未知操作: {}", other)),
    }
}

/// 执行用户【已批准】的文件操作（路径限制在游戏目录内，防穿越）
#[tauri::command]
async fn ai_execute_file_ops(ops: Vec<AiFileOp>, app_handle: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let game_dir = get_game_dir(&app_handle).await;
    let base = game_dir.canonicalize().unwrap_or(game_dir);
    let mut results = Vec::new();
    for op in &ops {
        results.push(apply_ai_file_op(&base, op)?);
    }
    Ok(serde_json::json!({ "results": results }))
}

/// 是否为 Anthropic 原生 Messages API（与 OpenAI 兼容格式不同）
fn ai_is_anthropic(provider: &str, base_url: &str) -> bool {
    provider == "anthropic" || base_url.contains("anthropic.com")
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiModelList {
    models: Vec<String>,
}

/// 获取当前 AI 提供商可用的模型列表。
/// OpenAI 兼容提供商走 GET /models；Anthropic 走 /v1/models。
#[tauri::command]
async fn list_ai_models(base_url: String, api_key: String, provider: String) -> Result<AiModelList, String> {
    let base = base_url.trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("创建请求失败: {}", e))?;

    let models: Vec<String> = if ai_is_anthropic(&provider, &base) {
        let origin = base.strip_suffix("/v1").unwrap_or(&base);
        let url = format!("{}/v1/models", origin);
        let resp = client
            .get(&url)
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .map_err(|e| format!("请求模型列表失败: {}", e))?;
        if !resp.status().is_success() {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("获取模型列表失败（HTTP {}）: {}", code, truncate_str(&text, 300)));
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| format!("解析模型列表失败: {}", e))?;
        v["data"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())
            .unwrap_or_default()
    } else {
        let url = format!("{}/models", base);
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .send()
            .await
            .map_err(|e| format!("请求模型列表失败（请检查 API 地址与网络）: {}", e))?;
        if !resp.status().is_success() {
            let code = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("获取模型列表失败（HTTP {}）: {}", code, truncate_str(&text, 300)));
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| format!("解析模型列表失败: {}", e))?;
        v["data"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())
            .unwrap_or_default()
    };

    Ok(AiModelList { models })
}

/// 单次调用 Anthropic Messages API（流式 SSE），返回（文本回复, 工具调用, 用量）。
/// 与 OpenAI 兼容接口差异：x-api-key 认证、system 顶层字段、工具格式 input_schema、内容块流式事件。
async fn ai_call_anthropic_streaming(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
    model: &str,
    system: &str,
    messages: &[serde_json::Value],
    tools: &serde_json::Value,
    app_handle: &tauri::AppHandle,
) -> Result<(String, Vec<AiToolCall>, AiUsage), String> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 8192,
        "system": system,
        "messages": messages,
        "tools": tools,
        "temperature": 0.3,
        "stream": true,
    });
    let resp = client
        .post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Accept", "text/event-stream")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求 AI 服务失败（请检查网络与 API 地址）: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("AI 服务返回错误（HTTP {}）: {}", status, truncate_str(&text, 300)));
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut content = String::new();
    let mut usage = AiUsage::default();
    // tool_use 累积：index -> AiToolCall（input 的 partial_json 用字符串累积）
    let mut tools_map: std::collections::BTreeMap<usize, AiToolCall> = std::collections::BTreeMap::new();
    let mut done = false;

    while !done {
        let chunk = tokio::time::timeout(Duration::from_secs(60), stream.next())
            .await
            .map_err(|_| "AI 响应超时（长时间无输出）".to_string())?;
        let chunk = match chunk {
            Some(Ok(c)) => c,
            Some(Err(e)) => return Err(format!("读取 AI 响应失败: {}", e)),
            None => break,
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();
            let line = line.trim();
            if line.is_empty() || !line.starts_with("data:") {
                continue;
            }
            let data = line[5..].trim();
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            match v["type"].as_str() {
                Some("message_start") => {
                    if let Some(it) = v["message"]["usage"]["input_tokens"].as_u64() {
                        usage.prompt_tokens = it;
                    }
                }
                Some("message_delta") => {
                    if let Some(ot) = v["usage"]["output_tokens"].as_u64() {
                        usage.completion_tokens = ot;
                    }
                }
                Some("content_block_start") => {
                    let idx = v["index"].as_u64().unwrap_or(0) as usize;
                    let cb = &v["content_block"];
                    if cb["type"] == "tool_use" {
                        let entry = tools_map.entry(idx).or_insert_with(|| AiToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: serde_json::Value::String(String::new()),
                        });
                        if let Some(id) = cb["id"].as_str() {
                            entry.id = id.to_string();
                        }
                        if let Some(name) = cb["name"].as_str() {
                            entry.name = name.to_string();
                        }
                    }
                }
                Some("content_block_delta") => {
                    let idx = v["index"].as_u64().unwrap_or(0) as usize;
                    let delta = &v["delta"];
                    match delta["type"].as_str() {
                        Some("text_delta") => {
                            if let Some(c) = delta["text"].as_str() {
                                if !c.is_empty() {
                                    content.push_str(c);
                                    let _ = app_handle.emit("ai-stream", json!({ "kind": "content", "text": c }));
                                }
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(pj) = delta["partial_json"].as_str() {
                                let entry = tools_map.entry(idx).or_insert_with(|| AiToolCall {
                                    id: String::new(),
                                    name: String::new(),
                                    arguments: serde_json::Value::String(String::new()),
                                });
                                let current = match &entry.arguments {
                                    serde_json::Value::String(s) => s.clone(),
                                    _ => String::new(),
                                };
                                entry.arguments = serde_json::Value::String(current + pj);
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_stop") => {
                    done = true;
                    break;
                }
                _ => {}
            }
        }
    }
    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    let mut out_calls = Vec::new();
    for (_, mut c) in tools_map {
        if let serde_json::Value::String(s) = &c.arguments {
            c.arguments = serde_json::from_str(s).unwrap_or(serde_json::json!({}));
        }
        out_calls.push(c);
    }
    Ok((content, out_calls, usage))
}

/// Anthropic 版聊天：拆分 system 与对话消息 → 调用 Messages API（最多 6 轮工具循环）。
fn ai_anthropic_tools() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "list_dir",
            "description": "列出游戏目录内某个目录的内容（相对路径，如 logs 或 versions）。用于自行查找日志/文件。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对游戏目录的目录路径，空表示根目录" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "read_file",
            "description": "读取游戏目录内的文本文件内容（相对路径）。自动执行，无需用户批准。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "相对游戏目录的文件路径" },
                    "max_chars": { "type": "integer", "description": "最多读取字符数，默认 20000" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "modify_files",
            "description": "提议对游戏目录内的文件进行修改（删除或写入）。仅提议，绝不执行，等待用户批准。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "operations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": { "type": "string", "enum": ["delete_file", "write_file"] },
                                "path": { "type": "string", "description": "相对游戏目录的路径" },
                                "content": { "type": "string", "description": "write_file 时要写入的内容" }
                            },
                            "required": ["action", "path"]
                        }
                    }
                },
                "required": ["operations"]
            }
        },
        {
            "name": "web_search",
            "description": "联网搜索，返回相关网页的标题、摘要与链接。用于获取模型本身不知道的信息（如最新资讯、报错解决方案）。自动执行，无需用户批准。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "搜索关键词" }
                },
                "required": ["query"]
            }
        }
    ])
}

async fn ai_chat_anthropic(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    full: &[serde_json::Value],
    used_wiki: bool,
    app_handle: &tauri::AppHandle,
    base: &Path,
) -> Result<AiChatResult, String> {
    let origin = base_url.trim_end_matches('/').trim_end_matches("/v1");
    let url = format!("{}/v1/messages", origin);

    // 拆分 system（顶层字段）与 user/assistant 对话消息
    let mut system_parts: Vec<String> = Vec::new();
    let mut messages: Vec<serde_json::Value> = Vec::new();
    for m in full {
        let role = m["role"].as_str().unwrap_or("user");
        let content = m["content"].as_str().unwrap_or("");
        if role == "system" {
            system_parts.push(content.to_string());
        } else {
            messages.push(serde_json::json!({ "role": role, "content": content }));
        }
    }
    let system = system_parts.join("\n\n");

    let mut reply = String::new();
    let mut usage_total = AiUsage::default();
    let mut tool_calls_out: Vec<AiToolCall> = Vec::new();
    let tools = ai_anthropic_tools();

    for _turn in 0..6 {
        let (r, calls, usage) = ai_call_anthropic_streaming(
            client, &url, api_key, model, &system, &messages, &tools, app_handle,
        )
        .await?;
        // 累加每轮的 token 用量（与 OpenAI 路径一致）
        usage_total.prompt_tokens += usage.prompt_tokens;
        usage_total.completion_tokens += usage.completion_tokens;
        usage_total.total_tokens += usage.total_tokens;
        reply.push_str(&r);

        if calls.is_empty() {
            break;
        }
        // 出现 modify_files → 收集给前端审批并停止循环
        let modify_calls: Vec<&AiToolCall> = calls.iter().filter(|c| c.name == "modify_files").collect();
        if !modify_calls.is_empty() {
            tool_calls_out.extend(modify_calls.into_iter().cloned());
            break;
        }

        // 构造 assistant 消息（文本 + tool_use 块）与 user 消息（tool_result 块）
        let mut assistant_blocks: Vec<serde_json::Value> = Vec::new();
        if !r.is_empty() {
            assistant_blocks.push(serde_json::json!({ "type": "text", "text": r }));
        }
        let mut tool_result_blocks: Vec<serde_json::Value> = Vec::new();
        for c in &calls {
            assistant_blocks.push(serde_json::json!({
                "type": "tool_use",
                "id": c.id,
                "name": c.name,
                "input": c.arguments
            }));
            let arg = if c.name == "web_search" {
                c.arguments["query"].as_str().unwrap_or("").to_string()
            } else {
                c.arguments["path"].as_str().unwrap_or("").to_string()
            };
            let _ = app_handle.emit("ai-stream", json!({ "kind": "status", "tool": c.name, "path": arg }));
            let result = match c.name.as_str() {
                "list_dir" => exec_list_dir(base, &c.arguments),
                "read_file" => exec_read_file(base, &c.arguments),
                "web_search" => exec_web_search(client, c.arguments["query"].as_str().unwrap_or("")).await,
                other => Err(format!("未知工具: {}", other)),
            };
            let tool_content = match result {
                Ok(v) => v,
                Err(e) => format!("错误: {}", e),
            };
            eprintln!("[ai-tool] {} {:?} -> {}", c.name, c.arguments, truncate_str(&tool_content, 300));
            tool_result_blocks.push(serde_json::json!({
                "type": "tool_result",
                "tool_use_id": c.id,
                "content": tool_content
            }));
        }
        messages.push(serde_json::json!({ "role": "assistant", "content": assistant_blocks }));
        messages.push(serde_json::json!({ "role": "user", "content": tool_result_blocks }));
    }

    Ok(AiChatResult { reply, used_wiki, usage: usage_total, tool_calls: tool_calls_out })
}

// ========== 4. 启动游戏 ==========
/// 大陆 IP 判断的进程内缓存：查询成功才缓存（网络错误不缓存，下次重试）
static MAINLAND_IP: std::sync::OnceLock<std::sync::Mutex<Option<bool>>> =
    std::sync::OnceLock::new();

/// 解析 Cloudflare trace 响应体，判断 loc 是否为中国大陆
fn trace_body_is_cn(body: &str) -> bool {
    body.lines().any(|l| l.trim() == "loc=CN")
}

/// 当前公网 IP 是否位于中国大陆。
/// 用 Cloudflare trace（无 key、轻量、全球可达）取 loc 字段。
/// 返回 Some(true)=大陆 / Some(false)=非大陆 / None=查询失败（断网等，无法判断）。
async fn ip_in_mainland_china() -> Option<bool> {
    let cache = MAINLAND_IP.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some(v) = *guard {
            return Some(v);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default();
    let resp = client
        .get("https://www.cloudflare.com/cdn-cgi/trace")
        .send()
        .await;
    let body = match resp {
        Ok(r) => r.text().await.ok(),
        Err(_) => None,
    };
    match body {
        Some(body) => {
            let is_cn = trace_body_is_cn(&body);
            if let Ok(mut guard) = cache.lock() {
                *guard = Some(is_cn);
            }
            Some(is_cn)
        }
        // 查询失败（无网络等）：不缓存（下次重试），返回 None（无法判断地区）
        None => None,
    }
}

/// 离线模式是否允许（Mojang 许可条款合规，供前端展示）：
/// 中国大陆一直允许（无需正版）；非大陆或断网无法判断地区时，仅当曾用正版启动过一次才允许。
#[tauri::command]
async fn offline_mode_allowed(app_handle: tauri::AppHandle) -> bool {
    let state = app_handle.state::<AppState>();
    let ip = ip_in_mainland_china().await; // 网络判定不持有配置锁
    let mut cfg = state.config.lock().await;
    let is_mainland = match ip {
        Some(is_cn) => {
            if cfg.known_mainland != Some(is_cn) {
                cfg.known_mainland = Some(is_cn);
                if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
                    let _ = fs::write(&state.config_path, json);
                }
            }
            is_cn
        }
        // 断网/不可达：回落到上次成功判定的地区；从未判定过则视为非大陆
        None => cfg.known_mainland.unwrap_or(false),
    };
    is_mainland || cfg.ever_launched_online
}

// ========== 检测指定 PID 的进程是否已出现可见窗口（用于启动动画「等游戏窗口出现再消失」） ==========
// 各平台实现：
//   macOS   : CoreGraphics CGWindowListCopyWindowInfo 枚举屏幕窗口，按 owner PID + layer == 0 过滤
//   Windows : EnumWindows + GetWindowThreadProcessId + IsWindowVisible
//   Linux   : X11 _NET_CLIENT_LIST + _NET_WM_PID + map_state == Viewable
// 非上述平台或检测不可用时返回 true，避免启动动画永久卡住（由超时兜底）。

#[cfg(target_os = "windows")]
fn pid_has_visible_window(pid: u32) -> bool {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::core::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Ctx {
        target: u32,
        found: bool,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lparam.0 as *mut Ctx);
            if ctx.found {
                return BOOL(0); // 已找到，停止枚举
            }
            let mut wpid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut wpid));
            if wpid == ctx.target && IsWindowVisible(hwnd).as_bool() {
                ctx.found = true;
                return BOOL(0);
            }
            BOOL(1) // 继续枚举
        }
    }

    let mut ctx = Ctx { target: pid, found: false };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.found
}

#[cfg(target_os = "macos")]
fn pid_has_visible_window(pid: u32) -> bool {
    use core_foundation_sys::array::CFArrayGetCount;
    use core_foundation_sys::array::CFArrayGetValueAtIndex;
    use core_foundation_sys::base::CFRelease;
    use core_foundation_sys::base::CFTypeRef;
    use core_foundation_sys::dictionary::CFDictionaryGetValue;
    use core_foundation_sys::dictionary::CFDictionaryRef;
    use core_foundation_sys::number::CFNumberGetValue;
    use core_foundation_sys::number::CFNumberRef;
    use core_foundation_sys::number::kCFNumberSInt64Type;
    use core_graphics::window::{
        kCGNullWindowID, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly, kCGWindowOwnerPID, CGWindowListCopyWindowInfo,
    };
    use std::os::raw::c_void;

    unsafe {
        let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
        let arr = CGWindowListCopyWindowInfo(options, kCGNullWindowID);
        if arr.is_null() {
            return false;
        }

        let count = CFArrayGetCount(arr);
        let mut found = false;
        for i in 0..count {
            let dict = CFArrayGetValueAtIndex(arr, i) as CFDictionaryRef;
            if dict.is_null() {
                continue;
            }

            // 窗口所属进程 PID
            let pid_ptr = CFDictionaryGetValue(dict, kCGWindowOwnerPID as *const c_void);
            if pid_ptr.is_null() {
                continue;
            }
            let mut owner_pid: i64 = 0;
            if !CFNumberGetValue(
                pid_ptr as CFNumberRef,
                kCFNumberSInt64Type,
                &mut owner_pid as *mut i64 as *mut c_void,
            ) {
                continue;
            }
            if owner_pid as u32 != pid {
                continue;
            }

            // 窗口层级：0 为普通应用窗口（排除菜单栏/Dock 等系统元素）
            let layer_ptr = CFDictionaryGetValue(dict, kCGWindowLayer as *const c_void);
            let mut layer: i64 = 0;
            if !layer_ptr.is_null() {
                CFNumberGetValue(
                    layer_ptr as CFNumberRef,
                    kCFNumberSInt64Type,
                    &mut layer as *mut i64 as *mut c_void,
                );
            }
            if layer == 0 {
                found = true;
                break;
            }
        }

        CFRelease(arr as CFTypeRef);
        found
    }
}

#[cfg(target_os = "linux")]
fn pid_has_visible_window(pid: u32) -> bool {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, MapState, Window};

    let (conn, screen_num) = match x11rb::connect(None) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let root = conn.setup().roots[screen_num].root;

    let intern = |name: &[u8]| -> Option<u32> {
        conn.intern_atom(false, name)
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    };
    let net_client_list = match intern(b"_NET_CLIENT_LIST") {
        Some(a) => a,
        None => return false,
    };
    let net_wm_pid = match intern(b"_NET_WM_PID") {
        Some(a) => a,
        None => return false,
    };

    // 读取根窗口 _NET_CLIENT_LIST（当前所有客户端窗口 id 列表）
    let windows: Vec<u32> = match conn
        .get_property(false, root, net_client_list, AtomEnum::WINDOW, 0, u32::MAX)
    {
        Ok(reply) => match reply.reply() {
            Ok(r) => r.value32().map(|v| v.collect()).unwrap_or_default(),
            Err(_) => return false,
        },
        Err(_) => return false,
    };

    for w in windows {
        let win = w as Window;
        // 读取该窗口的 _NET_WM_PID
        let win_pid = match conn
            .get_property(false, win, net_wm_pid, AtomEnum::CARDINAL, 0, 1)
        {
            Ok(reply) => match reply.reply() {
                Ok(r) => match r.value32().and_then(|mut it| it.next()) {
                    Some(p) => p,
                    None => continue,
                },
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if win_pid != pid {
            continue;
        }
        // 确认窗口已映射（可见）
        if let Ok(reply) = conn.get_window_attributes(win) {
            if let Ok(attr) = reply.reply() {
                if attr.map_state == MapState::VIEWABLE {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn pid_has_visible_window(_pid: u32) -> bool {
    true
}

#[tauri::command]
async fn launch_game(
    version: String,
    username: String,
    java_path: Option<String>,
    max_memory: Option<u32>,
    app_handle: tauri::AppHandle,
    window: Window,
) -> Result<(), String> {
    eprintln!("========== launch_game 被调用 ==========");
    eprintln!("version: '{}', username: '{}'", version, username);
    
    if version.trim().is_empty() {
        return Err("版本号不能为空".to_string());
    }
    if username.trim().is_empty() {
        return Err("玩家名不能为空".to_string());
    }

    let state = app_handle.state::<AppState>();
    let game_dir = get_game_dir(&app_handle).await;
    eprintln!("game_dir: {:?}", game_dir);
    
    let ip_region = ip_in_mainland_china().await;
    let mut config = state.config.lock().await;
    eprintln!("配置锁已获取");

    // ===== 合规检查：离线模式仅限中国大陆 IP 或曾正版启动过（Mojang 许可条款） =====
    // 判定标准与下方认证参数一致：未登录正版（无 ms_uuid / ms_access_token）即为离线启动
    let is_offline = config.ms_uuid.is_none() && config.ms_access_token.is_none();
    if is_offline {
        // 地区判定：查 IP 成功则持久化最近一次结果，断网则回落到 known_mainland
        let is_mainland = match ip_region {
            Some(is_cn) => {
                if config.known_mainland != Some(is_cn) {
                    config.known_mainland = Some(is_cn);
                    if let Ok(json) = serde_json::to_string_pretty(&*config) {
                        let _ = fs::write(&state.config_path, json);
                    }
                }
                is_cn
            }
            None => config.known_mainland.unwrap_or(false),
        };
        // 大陆：离线一直开放，无需正版；非大陆/断网但曾正版启动过：也开放
        if !is_mainland && !config.ever_launched_online {
            return Err(
                "当前网络地区不允许离线登录（Mojang 许可条款限制）。请使用 Microsoft 正版账号登录并至少启动一次游戏后，即可离线游玩。"
                    .to_string(),
            );
        }
    } else if !config.ever_launched_online {
        // 正版账号启动：记录 flag 并尽力落盘（失败不阻塞启动，之后即便断网/非大陆地区也能离线游玩）
        config.ever_launched_online = true;
        if let Ok(json) = serde_json::to_string_pretty(&*config) {
            let _ = fs::write(&state.config_path, json);
        }
    }

    // ===== 离线/正版认证参数 =====
    // 离线模式：accessToken 用 uuid 而非 "null"，否则 Minecraft 会进入 demo 试玩版（PCL2 做法）
    let auth_uuid = config.ms_uuid.clone().unwrap_or_else(|| offline_uuid(&username));
    let auth_access_token = config.ms_access_token.clone().unwrap_or(auth_uuid.clone());
    // 与 PCL 等启动器一致：离线账户同样使用 msa。现代版本会据此解释
    // 启动参数；mojang 会造成不兼容的账号状态。
    let auth_user_type = "msa";

    let versions_dir = game_dir.join("versions").join(&version);
    let json_path = versions_dir.join(format!("{}.json", version));
    eprintln!("json_path: {:?}", json_path);

    if !json_path.exists() {
        eprintln!("版本 JSON 不存在");
        return Err(format!("版本 {} 未安装，请先下载", version));
    }

    // ===== 读取版本 JSON =====
    let json_str = fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
    let version_json: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| e.to_string())?;
    let version_id = version_json["id"].as_str().unwrap_or(&version);

    // ===== 处理 inheritsFrom 继承链 =====
    let mut merged_libraries = Vec::new();
    let mut client_jar_path = None;
    let mut asset_id = String::from("legacy");
    let mut native_paths = Vec::new();

    // 收集所有要处理的版本 JSON（从父到子）
    let mut version_chain = Vec::new();
    version_chain.push(version_json.clone());

    // 检查 inheritsFrom
    if let Some(inherits_from) = version_json["inheritsFrom"].as_str() {
        let parent_version_dir = game_dir.join("versions").join(inherits_from);
        let parent_json_path = parent_version_dir.join(format!("{}.json", inherits_from));
        if parent_json_path.exists() {
            let parent_json_str = fs::read_to_string(&parent_json_path).map_err(|e| e.to_string())?;
            let parent_json: serde_json::Value = serde_json::from_str(&parent_json_str).map_err(|e| e.to_string())?;
            // 父版本放前面，子版本后面（子版本优先覆盖）
            version_chain.insert(0, parent_json);
        }
    }

    // 处理每个版本的 libraries、client jar、assetIndex
    for (idx, ver_json) in version_chain.iter().enumerate() {
        let is_parent = idx < version_chain.len() - 1; // 最后一个是当前版本

        // 收集 libraries
        if let Some(libraries) = ver_json["libraries"].as_array() {
            for lib in libraries {
                // 方式1: 有 downloads.artifact 的标准格式
                if let Some(artifact) = lib["downloads"]["artifact"].as_object() {
                    if let Some(path) = artifact["path"].as_str() {
                        let lib_path = game_dir.join("libraries").join(path);
                        if lib_path.exists() {
                            let path_str = lib_path.to_str().unwrap().to_string();
                            if !merged_libraries.contains(&path_str) {
                                merged_libraries.push(path_str);
                            }
                        }
                    }
                }
                // 方式2: 只有 name 字段的格式 (Fabric/Forge loader libraries)
                else if let Some(name) = lib["name"].as_str() {
                    // 解析 Maven 坐标: group:artifact:version[:classifier]
                    // 参考 PCL2 McLibGet：文件夹忽略最后一段（classifier），文件名保留
                    // 例: net.minecraftforge:forge:1.21.4-54.0.34:client
                    let parts: Vec<&str> = name.split(':').collect();
                    if parts.len() >= 3 {
                        let group_path = parts[0].replace('.', "/");
                        let artifact = parts[1];
                        let version = parts[2];
                        let file_name = if parts.len() == 4 {
                            format!("{}-{}-{}.jar", artifact, version, parts[3])
                        } else {
                            format!("{}-{}.jar", artifact, version)
                        };
                        let lib_path_str = format!("{}/{}/{}/{}", group_path, artifact, version, file_name);
                        let lib_path = game_dir.join("libraries").join(&lib_path_str);
                        if lib_path.exists() {
                            let path_str = lib_path.to_str().unwrap().to_string();
                            if !merged_libraries.contains(&path_str) {
                                merged_libraries.push(path_str);
                            }
                        }
                    }
                }
            }
        }

        // 收集 client jar
        if !is_parent {
            // 对于当前版本，先尝试用版本 ID 找 jar
            let jar_name = ver_json["id"].as_str().unwrap_or(&version);
            let jar_path = versions_dir.join(format!("{}.jar", jar_name));
            if jar_path.exists() {
                client_jar_path = Some(jar_path.to_str().unwrap().to_string());
            } else {
                // 尝试用 version 参数名
                let alt_jar_path = versions_dir.join(format!("{}.jar", version));
                if alt_jar_path.exists() {
                    client_jar_path = Some(alt_jar_path.to_str().unwrap().to_string());
                }
            }
        } else if client_jar_path.is_none() {
            // 对于父版本，用父版本的 ID 找 jar
            if let Some(parent_id) = ver_json["id"].as_str() {
                let parent_dir_name = ver_json["id"].as_str().unwrap_or("");
                let parent_jar_path = game_dir.join("versions").join(parent_dir_name).join(format!("{}.jar", parent_id));
                if parent_jar_path.exists() {
                    client_jar_path = Some(parent_jar_path.to_str().unwrap().to_string());
                }
            }
        }

        // 获取 assetIndex（优先使用子版本的）
        if !is_parent || asset_id == "legacy" {
            if let Some(asset_idx) = ver_json["assetIndex"].as_object() {
                if let Some(id) = asset_idx["id"].as_str() {
                    asset_id = id.to_string();
                }
            }
        }
    }

    // 收集 natives 路径
    let natives_dir = versions_dir.join("natives");
    if natives_dir.exists() {
        native_paths.push(natives_dir.to_str().unwrap().to_string());
    }
    
    // 检查父版本的 natives（用于加载器版本）
    if let Some(inherits_from) = version_json["inheritsFrom"].as_str() {
        let parent_natives_dir = game_dir.join("versions").join(inherits_from).join("natives");
        if parent_natives_dir.exists() {
            native_paths.push(parent_natives_dir.to_str().unwrap().to_string());
        }
    }

    // ===== 生成最小配置，强制 tutorialStep:none 跳过向导 =====
    let options_content = format!(
        r#"tutorialStep:none
version:{}
lang:zh_cn
launcher_name:Lumia
launcher_version:0.1.0
"#,
        version_id
    );

    // 写入全局 options.txt（覆盖已有文件，确保 tutorialStep:none 生效）
    let global_options_path = game_dir.join("options.txt");
    if let Err(e) = fs::write(&global_options_path, &options_content) {
        eprintln!("写入全局 options.txt 失败: {}", e);
    }

    // 写入版本目录下的 options.txt
    let version_options_path = versions_dir.join("options.txt");
    if let Err(e) = fs::write(&version_options_path, &options_content) {
        eprintln!("写入版本 options.txt 失败: {}", e);
    }

    // ===== 调试日志 =====
    eprintln!("========== 启动调试信息 ==========");
    eprintln!("版本: {}", version);
    eprintln!("版本目录: {:?}", versions_dir);
    eprintln!("gameDir (全局): {:?}", game_dir);
    eprintln!("inheritsFrom: {:?}", version_json["inheritsFrom"].as_str());
    eprintln!("version_id: {}", version_id);

    // 收集 mainClass - 优先使用当前版本的，否则从父版本获取
    let main_class;
    if let Some(mc) = version_json["mainClass"].as_str() {
        main_class = mc.to_string();
    } else if let Some(inherits_from) = version_json["inheritsFrom"].as_str() {
        let parent_json_path = game_dir.join("versions").join(inherits_from).join(format!("{}.json", inherits_from));
        if parent_json_path.exists() {
            if let Ok(parent_json_str) = fs::read_to_string(&parent_json_path) {
                if let Ok(parent_json) = serde_json::from_str::<serde_json::Value>(&parent_json_str) {
                    main_class = parent_json["mainClass"]
                        .as_str()
                        .unwrap_or("net.minecraft.client.main.Main")
                        .to_string();
                } else {
                    main_class = "net.minecraft.client.main.Main".to_string();
                }
            } else {
                main_class = "net.minecraft.client.main.Main".to_string();
            }
        } else {
            main_class = "net.minecraft.client.main.Main".to_string();
        }
    } else {
        main_class = "net.minecraft.client.main.Main".to_string();
    }
    eprintln!("mainClass: {}", main_class);

    // 收集 game arguments - 支持 arguments.game 数组格式和 minecraftArguments 字符串格式
    let mut game_args = Vec::new();

    let replace_game_argument = |template: &str| {
        template
            .replace("${auth_player_name}", &username)
            .replace("${version_name}", version_id)
            .replace("${game_directory}", versions_dir.to_str().unwrap())
            .replace("${assets_root}", game_dir.join("assets").to_str().unwrap())
            // 官方版本 JSON 使用 ${assets_index_name}（assets 带 s）；
            // 同时兼容少数旧 profile 的 ${asset_index_name} 写法。
            .replace("${asset_index}", &asset_id)
            .replace("${assets_index_name}", &asset_id)
            .replace("${asset_index_name}", &asset_id)
            .replace("${auth_uuid}", &auth_uuid)
            .replace("${auth_access_token}", &auth_access_token)
            .replace("${user_type}", auth_user_type)
            .replace("${version_type}", "release").replace("${auth_xuid}", "").replace("${clientid}", "")
    };
    let mut append_game_arguments = |args_array: &[serde_json::Value]| {
        for arg in args_array {
            if let Some(value) = arg.as_str() {
                game_args.push(replace_game_argument(value));
            } else if let Some(obj) = arg.as_object() {
                if !game_argument_allowed(obj.get("rules").and_then(|rules| rules.as_array())) {
                    continue;
                }
                if let Some(value) = obj.get("value").and_then(|value| value.as_str()) {
                    game_args.push(replace_game_argument(value));
                } else if let Some(values) = obj.get("value").and_then(|value| value.as_array()) {
                    for value in values.iter().filter_map(|value| value.as_str()) {
                        game_args.push(replace_game_argument(value));
                    }
                }
            }
        }
    };
    
    // 先检查当前版本是否有 arguments.game 数组格式
    if let Some(args_array) = version_json["arguments"]["game"].as_array() {
        eprintln!("使用 arguments.game 数组格式 (元素数量: {})", args_array.len());
        append_game_arguments(args_array);
    } else if let Some(minecraft_args) = version_json["minecraftArguments"].as_str() {
        eprintln!("使用 minecraftArguments 字符串格式");
        let mut result = minecraft_args.to_string();
        result = result.replace("${auth_player_name}", &username);
        result = result.replace("${version_name}", &version);
        result = result.replace("${game_directory}", versions_dir.to_str().unwrap());
        result = result.replace("${assets_root}", game_dir.join("assets").to_str().unwrap());
        result = result.replace("${asset_index}", &asset_id)
            .replace("${assets_index_name}", &asset_id)
            .replace("${asset_index_name}", &asset_id);
        result = result.replace("${auth_uuid}", &auth_uuid);
        result = result.replace("${auth_access_token}", &auth_access_token);
        result = result.replace("${user_type}", auth_user_type);
        result = result.replace("${version_type}", "release").replace("${auth_xuid}", "").replace("${clientid}", "");
        game_args = result.split(' ').map(|s| s.to_string()).collect();
    } else if let Some(inherits_from) = version_json["inheritsFrom"].as_str() {
        let parent_json_path = game_dir.join("versions").join(inherits_from).join(format!("{}.json", inherits_from));
        if parent_json_path.exists() {
            if let Ok(parent_json_str) = fs::read_to_string(&parent_json_path) {
                if let Ok(parent_json) = serde_json::from_str::<serde_json::Value>(&parent_json_str) {
                    if let Some(args_array) = parent_json["arguments"]["game"].as_array() {
                        eprintln!("从父版本获取 arguments.game 数组格式");
                        append_game_arguments(args_array);
                    } else if let Some(minecraft_args) = parent_json["minecraftArguments"].as_str() {
                        eprintln!("从父版本获取 minecraftArguments 字符串格式");
                        let mut result = minecraft_args.to_string();
                        result = result.replace("${auth_player_name}", &username);
                        result = result.replace("${version_name}", &version);
                        result = result.replace("${game_directory}", versions_dir.to_str().unwrap());
                        result = result.replace("${assets_root}", game_dir.join("assets").to_str().unwrap());
                        result = result.replace("${asset_index}", &asset_id)
                            .replace("${assets_index_name}", &asset_id)
                            .replace("${asset_index_name}", &asset_id);
                        result = result.replace("${auth_uuid}", &auth_uuid);
                        result = result.replace("${auth_access_token}", &auth_access_token);
                        result = result.replace("${user_type}", auth_user_type);
                        result = result.replace("${version_type}", "release").replace("${auth_xuid}", "").replace("${clientid}", "");
                        game_args = result.split(' ').map(|s| s.to_string()).collect();
                    }
                }
            }
        }
    }
    
    // 如果还是空的，使用默认参数
    if game_args.is_empty() {
        eprintln!("使用默认 game_args");
        game_args = vec![
            "--username".to_string(),
            username,
            "--version".to_string(),
            version.clone(),
            "--gameDir".to_string(),
            versions_dir.to_str().unwrap().to_string(),
            "--assetsDir".to_string(),
            game_dir.join("assets").to_str().unwrap().to_string(),
            "--assetIndex".to_string(),
            asset_id.to_string(),
            "--uuid".to_string(),
            auth_uuid.clone(),
            "--accessToken".to_string(),
            auth_access_token.clone(),
            "--userType".to_string(),
            auth_user_type.to_string(),
            "--lang".to_string(),
            "zh_cn".to_string(),
            "--narration".to_string(),
            "off".to_string(),
        ];
    }
    eprintln!("game_args 数量: {}", game_args.len());

    // NeoForge/Forge：加载器版本的原版 jar 必须从 classpath 排除。
    // 补丁后的 client jar（libraries 里的 neoforge/forge client 构件）已包含全部
    // 游戏类，若原版 jar 同时在场，modlauncher 模块化 classpath 时会报
    // "Modules _1._21._1 and minecraft export package net.minecraft.server" 冲突。
    // 检测：库名含 net.neoforged / net.minecraftforge 即视为加载器版本
    // （Fabric/Quilt 只有 net.fabricmc/net.quiltmc，不命中，保留原版 jar）。
    let has_loader_client = version_json["libraries"].as_array().map(|libs| {
        libs.iter().any(|l| {
            l["name"]
                .as_str()
                .map(|n| n.contains("net.neoforged") || n.contains("net.minecraftforge"))
                .unwrap_or(false)
        })
    }).unwrap_or(false);
    if has_loader_client {
        client_jar_path = None;
        eprintln!("检测到 Forge/NeoForge 加载器，从 classpath 排除原版 jar");
    }

    let mut classpath_entries = vec![];

    // 添加 client jar
    if let Some(client_jar) = client_jar_path {
        eprintln!("client jar: {}", client_jar);
        classpath_entries.push(client_jar);
    } else {
        eprintln!("警告: 未找到 client jar");
    }

    // 添加合并后的 libraries
    eprintln!("libraries 数量: {}", merged_libraries.len());
    for (i, lib) in merged_libraries.iter().enumerate() {
        if i < 5 || !std::path::Path::new(lib).exists() {
            eprintln!("  lib[{}]: {} (exists: {})", i, lib, std::path::Path::new(lib).exists());
        }
    }
    classpath_entries.extend(merged_libraries);

    let classpath = classpath_entries.join(if cfg!(windows) { ";" } else { ":" });
    eprintln!("classpath 总长度: {} 字符", classpath.len());

    // 合并 natives 路径
    let java_library_path = if native_paths.is_empty() {
        versions_dir.join("natives").to_str().unwrap_or("").to_string()
    } else {
        native_paths.join(if cfg!(windows) { ";" } else { ":" })
    };
    eprintln!("java_library_path: {}", java_library_path);

    let java_exec = java_path
        .or_else(|| config.java_path.clone())
        .or_else(detect_java_executable)
        .unwrap_or_else(|| "java".to_string());
    eprintln!("java_exec: {}", java_exec);

    let max_mem = max_memory.unwrap_or(config.max_memory);

    // ===== JVM 参数：从版本 json 的 arguments.jvm 读取并替换占位符 =====
    // 新版 Minecraft 的 natives 路径、native-access 等参数都在 json 里，必须处理
    let mut jvm_args: Vec<String> = Vec::new();
    let mut has_xmx = false;
    if let Some(jvm_array) = version_json["arguments"]["jvm"].as_array() {
        for arg in jvm_array {
            if let Some(s) = arg.as_str() {
                let val = s
                    .replace("${natives_directory}", &java_library_path)
                    .replace("${classpath}", &classpath)
                    .replace("${library_directory}", game_dir.join("libraries").to_str().unwrap_or(""))
                    .replace("${classpath_separator}", if cfg!(windows) { ";" } else { ":" })
                    .replace("${launcher_name}", "Lumia")
                    .replace("${launcher_version}", "0.1.0");
                if val.starts_with("-Xmx") {
                    has_xmx = true;
                    jvm_args.push(format!("-Xmx{}M", max_mem));
                } else {
                    jvm_args.push(val);
                }
            } else if let Some(obj) = arg.as_object() {
                // 带 rules 的参数：目前只处理 os.name == osx 的 allow 规则（macOS 场景）
                let rules = obj["rules"].as_array();
                let mut allowed = true;
                if let Some(rules) = rules {
                    allowed = false;
                    for rule in rules {
                        let action = rule["action"].as_str().unwrap_or("");
                        let os_name = rule["os"]["name"].as_str().unwrap_or("");
                        let os_arch = rule["os"]["arch"].as_str().unwrap_or("");
                        let is_macos = cfg!(target_os = "macos") && (os_name.is_empty() || os_name == "osx");
                        let is_windows = cfg!(target_os = "windows") && (os_name.is_empty() || os_name == "windows");
                        let is_arch_ok = os_arch.is_empty() ||
                            (cfg!(target_arch = "x86_64") && os_arch == "x86") ||
                            (cfg!(target_arch = "aarch64") && os_arch == "arm64");
                        let os_ok = is_macos || is_windows;
                        if action == "allow" && os_ok && is_arch_ok {
                            allowed = true;
                        }
                    }
                }
                if allowed {
                    if let Some(values) = obj["value"].as_array() {
                        for v in values {
                            if let Some(vs) = v.as_str() {
                                let val = vs
                                    .replace("${natives_directory}", &java_library_path)
                                    .replace("${classpath}", &classpath)
                                    .replace("${library_directory}", game_dir.join("libraries").to_str().unwrap_or(""))
                                    .replace("${classpath_separator}", if cfg!(windows) { ";" } else { ":" })
                                    .replace("${launcher_name}", "Lumia")
                                    .replace("${launcher_version}", "0.1.0");
                                jvm_args.push(val);
                            }
                        }
                    } else if let Some(vs) = obj["value"].as_str() {
                        let val = vs
                            .replace("${natives_directory}", &java_library_path)
                            .replace("${classpath}", &classpath)
                            .replace("${launcher_name}", "Lumia")
                            .replace("${launcher_version}", "0.1.0");
                        jvm_args.push(val);
                    }
                }
            }
        }
    }
    // json 没提供 jvm 参数时用兜底
    if jvm_args.is_empty() {
        jvm_args.push(format!("-Djava.library.path={}", java_library_path));
    }
    if !has_xmx {
        jvm_args.push(format!("-Xmx{}M", max_mem));
    }
    // ===== 用户自定义 Java 启动参数（设置 → 高级设置），按空白拆分成多个参数追加 =====
    if !config.java_args.trim().is_empty() {
        for tok in config.java_args.split_whitespace() {
            jvm_args.push(tok.to_string());
        }
    }
    eprintln!("jvm_args: {:?}", jvm_args);

    #[cfg(target_arch = "aarch64")]
    let use_rosetta = config.use_rosetta;
    #[cfg(not(target_arch = "aarch64"))]
    let use_rosetta = false;

    // 确保版本目录存在且包含必要的子目录
    let _ = fs::create_dir_all(&versions_dir);
    for subdir in &["mods", "saves", "resourcepacks", "screenshots", "logs", "crash-reports"] {
        let _ = fs::create_dir_all(versions_dir.join(subdir));
    }
    
    eprintln!("gameDir (版本专属): {:?}", versions_dir);
    eprintln!("========== 调试信息结束 ==========");

    let _ = window.emit("launch-status", json!({ "stage": "正在启动 Minecraft..." }));

    let mut cmd = Command::new(&java_exec);
    cmd.args(&jvm_args);
    // json 的 jvm 参数可能已包含 -cp ${classpath}（已替换），若无则手动加
    let has_cp = jvm_args.windows(2).any(|w| w[0] == "-cp");
    if !has_cp {
        cmd.arg("-cp").arg(&classpath);
    }
    cmd.arg(main_class.clone());
    cmd.args(&game_args);
    cmd.current_dir(&versions_dir);
    // Windows 上 java.exe 是控制台程序，直接启动会弹出黑色命令行窗口；
    // 用 CREATE_NO_WINDOW 隐藏，输出改接空设备（游戏自己写日志文件）
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
    }
    #[cfg(not(windows))]
    {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }

    let mut final_cmd = if use_rosetta {
        let mut rosetta_cmd = Command::new("arch");
        rosetta_cmd.arg("-x86_64");
        rosetta_cmd.arg(&java_exec);
        rosetta_cmd.args(&jvm_args);
        if !has_cp {
            rosetta_cmd.arg("-cp").arg(&classpath);
        }
        rosetta_cmd.arg(main_class);
        rosetta_cmd.args(&game_args);
        rosetta_cmd.current_dir(&versions_dir);
        #[cfg(not(windows))]
        {
            rosetta_cmd.stdout(Stdio::inherit());
            rosetta_cmd.stderr(Stdio::inherit());
        }
        rosetta_cmd
    } else {
        cmd
    };

    // 打印最终启动命令，便于确认用户自定义 Java 参数是否生效
    {
        let program = final_cmd.get_program().to_string_lossy().to_string();
        let args: Vec<String> = final_cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        eprintln!("[启动] 完整命令: {} {}", program, args.join(" "));
    }

    let mut child = final_cmd
        .spawn()
        .map_err(|e| {
            // Windows 上最常见：Java 未安装或不在 PATH（os error 2 / "program not found"）
            let hint = if cfg!(windows) {
                "（Windows 提示 program not found 通常是未安装 Java 或不在 PATH）"
            } else {
                ""
            };
            format!("启动游戏失败: {}{}\n请先安装 Java（26.2 需要 JDK 25，1.20~1.21 需要 JDK 21），或在设置中手动指定 Java 路径", e, hint)
        })?;

    let pid = child.id();
    GAME_RUNNING.store(true, std::sync::atomic::Ordering::SeqCst);

    // 进程存活标记：窗口检测任务据此判断进程是否已提前退出（避免与退出事件竞争）
    let process_alive = Arc::new(AtomicBool::new(true));

    // 退出监听任务
    {
        let process_alive = process_alive.clone();
        let window = window.clone();
        tauri::async_runtime::spawn(async move {
            let status = child.wait();
            process_alive.store(false, Ordering::SeqCst);
            GAME_RUNNING.store(false, Ordering::SeqCst);
            match status {
                Ok(exit_code) => {
                    let _ = window.emit("launch-status", json!({
                        "stage": "游戏已退出",
                        "exit_code": exit_code.code().unwrap_or(-1)
                    }));
                }
                Err(e) => {
                    let _ = window.emit("launch-status", json!({
                        "stage": "游戏异常退出",
                        "error": e.to_string()
                    }));
                }
            }
        });
    }

    // 窗口检测任务：等待游戏窗口真正出现后才发「游戏已启动」，
    // 让前端启动动画持续到窗口可见而不是进程 spawn 成功。
    {
        let process_alive = process_alive.clone();
        let window = window.clone();
        tauri::async_runtime::spawn(async move {
            let started = Instant::now();
            let timeout = Duration::from_secs(90); // 兜底：90s 仍未检测到窗口也视为已启动，避免动画永久卡住
            loop {
                if !process_alive.load(Ordering::SeqCst) {
                    // 进程已退出，退出任务会发「游戏已退出/异常退出」，这里无需再发
                    break;
                }
                if pid_has_visible_window(pid) {
                    let _ = window.emit("launch-status", json!({ "stage": "游戏已启动", "pid": pid }));
                    break;
                }
                if started.elapsed() >= timeout {
                    let _ = window.emit("launch-status", json!({ "stage": "游戏已启动", "pid": pid }));
                    break;
                }
                sleep(Duration::from_millis(200)).await;
            }
        });
    }

    Ok(())
}

// ========== 5. 打开游戏目录 ==========
#[tauri::command]
async fn open_folder(app_handle: tauri::AppHandle) -> Result<(), String> {
    let game_dir = get_game_dir(&app_handle).await;
    if !game_dir.exists() {
        fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;
    }
    open::that(&game_dir).map_err(|e| e.to_string())
}

// ========== 5. 打开崩溃日志文件 ==========
#[tauri::command]
async fn open_crash_log_file(app_handle: tauri::AppHandle) -> Result<(), String> {
    let game_dir = get_game_dir(&app_handle).await;
    let versions_root = game_dir.join("versions");

    let candidates = find_latest_crash_report(&versions_root);
    if let Some(p) = candidates.first() {
        return open::that(p).map_err(|e| format!("打开崩溃日志失败: {}", e));
    }

    let logs = find_latest_log(&versions_root);
    if let Some(p) = logs.first() {
        return open::that(p).map_err(|e| format!("打开游戏日志失败: {}", e));
    }

    Err("未找到崩溃日志或游戏日志".to_string())
}

fn find_latest_crash_report(versions_root: &Path) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = fs::read_dir(versions_root) {
        for entry in rd.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let crash_dir = dir.join("crash-reports");
            if let Ok(crd) = fs::read_dir(&crash_dir) {
                for c in crd.flatten() {
                    let p = c.path();
                    if p.extension().and_then(|e| e.to_str()) == Some("txt") {
                        candidates.push(p);
                    }
                }
            }
        }
    }
    candidates.sort_by(|a, b| {
        let at = a.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let bt = b.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        bt.cmp(&at)
    });
    candidates
}

fn find_latest_log(versions_root: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = fs::read_dir(versions_root) {
        for entry in rd.flatten() {
            let log = entry.path().join("logs").join("latest.log");
            if log.exists() {
                logs.push(log);
            }
        }
    }
    logs.sort_by(|a, b| {
        let at = a.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let bt = b.metadata().and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        bt.cmp(&at)
    });
    logs
}

#[derive(Serialize, Debug, Clone)]
struct CrashInfo {
    description: String,
    entity_type: Option<String>,
    entity_name: Option<String>,
    entity_location: Option<String>,
    block_type: Option<String>,
    block_location: Option<String>,
    error_type: String,
    stack_summary: String,
    raw_section: Option<String>,
}

fn find_section_range<'a>(lines: &[&'a str], start_marker: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| l.trim().starts_with(start_marker))?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim().starts_with("--") || l.trim().is_empty())
        .map(|p| start + 1 + p)
        .unwrap_or(lines.len());
    Some((start + 1, end))
}

fn extract_value<'a>(lines: &[&'a str], prefix: &str) -> Option<String> {
    for line in lines {
        let trimmed = line.trim();
        if let Some(v) = trimmed.strip_prefix(prefix) {
            let val = v.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn parse_crash_report_text(content: &str) -> CrashInfo {
    let lines: Vec<&str> = content.lines().collect();

    let description = lines
        .iter()
        .find(|l| l.trim().starts_with("Description:"))
        .and_then(|l| l.trim().strip_prefix("Description:"))
        .map(|d| d.trim().to_string())
        .unwrap_or_else(|| "未知错误".to_string());

    let entity_section = find_section_range(&lines, "-- Entity being ticked --");
    let block_section = find_section_range(&lines, "-- Block being ticked --");
    let level_section = find_section_range(&lines, "-- Affected level --");

    let mut entity_type: Option<String> = None;
    let mut entity_name: Option<String> = None;
    let mut entity_location: Option<String> = None;

    if let Some((start, end)) = entity_section {
        let sec = &lines[start..end.min(lines.len())];
        entity_type = extract_value(sec, "Entity Type:");
        entity_name = extract_value(sec, "Entity Name:");
        entity_location = extract_value(sec, "Entity's Exact location:");
    }

    let mut block_type: Option<String> = None;
    let mut block_location: Option<String> = None;

    if let Some((start, end)) = block_section {
        let sec = &lines[start..end.min(lines.len())];
        block_type = extract_value(sec, "Block:");
        block_location = extract_value(sec, "Location:");
    }

    // 如果方块坐标没解析到，尝试从 Affected level 的 Level location 获取
    if block_location.is_none() {
        if let Some((start, end)) = level_section {
            let sec = &lines[start..end.min(lines.len())];
            block_location = extract_value(sec, "Level location:");
        }
    }

    // 错误类型分类
    let error_type = {
        let desc_lower = description.to_lowercase();
        if desc_lower.contains("ticking entity") || desc_lower.contains("ticking block entity") {
            if entity_type.is_some() {
                "entity_ticking"
            } else {
                "ticking"
            }
        } else if desc_lower.contains("ticking block") || block_type.is_some() {
            "block_ticking"
        } else if desc_lower.contains("out of memory") || desc_lower.contains("java heap space") {
            "out_of_memory"
        } else if desc_lower.contains("java.lang.unsatisfiedlinkerror")
            || desc_lower.contains("java.lang.noclassdeffounderror")
        {
            "missing_native"
        } else if desc_lower.contains("java.lang.nosuchmethoderror")
            || desc_lower.contains("java.lang.classnotfoundexception")
        {
            "version_mismatch"
        } else if desc_lower.contains("failed to create window")
            || desc_lower.contains("pixel format not accelerated")
        {
            "gpu_error"
        } else if desc_lower.contains("stack overflow") || desc_lower.contains("stackoverflowerror") {
            "stack_overflow"
        } else if desc_lower.contains("null pointer") || desc_lower.contains("nullpointerexception") {
            "null_pointer"
        } else {
            "generic"
        }
    };

    // 提取堆栈摘要
    let mut stack_summary = String::new();
    let mut found_at = false;
    let mut count = 0;
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("at ") {
            found_at = true;
            if count < 5 {
                stack_summary.push_str(trimmed);
                stack_summary.push('\n');
                count += 1;
            }
        } else if found_at && !trimmed.starts_with("at ") && !trimmed.starts_with("...") {
            if count >= 5 {
                break;
            }
        }
    }
    if stack_summary.is_empty() {
        stack_summary = description.clone();
    }

    // 标记原始 section 用于前端展示
    let raw_section = if entity_section.is_some() || block_section.is_some() {
        Some("details".to_string())
    } else {
        None
    };

    CrashInfo {
        description,
        entity_type,
        entity_name,
        entity_location,
        block_type,
        block_location,
        error_type: error_type.to_string(),
        stack_summary,
        raw_section,
    }
}

#[tauri::command]
async fn parse_crash_report(app_handle: tauri::AppHandle) -> Result<CrashInfo, String> {
    let game_dir = get_game_dir(&app_handle).await;
    let versions_root = game_dir.join("versions");

    let candidates = find_latest_crash_report(&versions_root);
    if let Some(p) = candidates.first() {
        let content = fs::read_to_string(p).map_err(|e| format!("读取崩溃报告失败: {}", e))?;
        return Ok(parse_crash_report_text(&content));
    }

    // 兜底：读取 latest.log
    let logs = find_latest_log(&versions_root);
    if let Some(p) = logs.first() {
        let content = fs::read_to_string(p).map_err(|e| format!("读取游戏日志失败: {}", e))?;
        return Ok(parse_crash_report_text(&content));
    }

    Err("未找到崩溃日志或游戏日志".to_string())
}

// ========== 6. 获取配置 ==========
#[tauri::command]
async fn get_config(app_handle: tauri::AppHandle) -> Result<AppConfig, String> {
    let state = app_handle.state::<AppState>();
    let config = state.config.lock().await;
    Ok(config.clone())
}

// ========== 7. 保存配置 ==========
#[tauri::command]
async fn save_config(
    config: AppConfig,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let mut current = state.config.lock().await;
    *current = config.clone();

    let config_path = &state.config_path;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(config_path, json).map_err(|e| e.to_string())?;

    Ok(())
}

// ========== 7.05 自定义背景图管理 ==========
/// 返回 Lumia 数据目录（跨平台）
fn lumia_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join("Library/Application Support/Lumia")
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("Lumia")
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".local/share/Lumia")
    }
}

/// 将 base64 图片解码并保存到 Lumia 数据目录，返回文件路径
#[tauri::command]
async fn save_background_image(
    base64_data: String,
    file_name: String,
) -> Result<String, String> {
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD
        .decode(&base64_data)
        .map_err(|e| format!("解码图片失败: {}", e))?;

    let lumia_dir = lumia_data_dir();
    fs::create_dir_all(&lumia_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    let ext = Path::new(&file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let filename = format!(".lumia_background.{}", ext);
    let dest = lumia_dir.join(&filename);

    fs::write(&dest, &data).map_err(|e| format!("写入图片失败: {}", e))?;

    Ok(dest.to_string_lossy().to_string())
}

/// 删除指定路径的背景图文件
#[tauri::command]
async fn remove_background_image(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if p.exists() {
        fs::remove_file(p).map_err(|e| format!("删除图片失败: {}", e))?;
    }
    Ok(())
}

// ========== 7.06 工具箱 - 32线程快速下载器 ==========
#[tauri::command]
async fn download_file(
    url: String,
    save_path: String,
    window: tauri::Window,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .connect_timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(64)
        .tcp_nodelay(true)
        .build()
        .map_err(|e| format!("创建客户端失败: {}", e))?;

    let dest = PathBuf::from(&save_path);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let _ = window.emit("download-file-progress", json!({ "stage": "正在获取文件信息...", "progress": 0 }));

    // 解析 URL 获取 host，用作 Referer 绕过防盗链
    let referer = if let Ok(parsed) = reqwest::Url::parse(&url) {
        format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""))
    } else {
        url.clone()
    };

    // 先尝试 HEAD 获取文件大小和 Range 支持
    let head_req = client
        .head(&url)
        .header("Referer", &referer);
    let head_resp = match head_req.send().await {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("HEAD 请求失败 (将回退到 GET): {}", e);
            None
        }
    };

    let mut total_size: u64 = 0;
    let mut accept_ranges = false;

    if let Some(ref resp) = head_resp {
        if resp.status().is_success() || resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            total_size = resp.content_length().unwrap_or(0);
            accept_ranges = resp
                .headers()
                .get("accept-ranges")
                .map(|v| v.to_str().unwrap_or("") == "bytes")
                .unwrap_or(false);
        } else if resp.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            eprintln!("HEAD 不被支持 (405)，回退到 GET 探测");
        } else {
            eprintln!("HEAD 返回 {}，回退到 GET 探测", resp.status());
        }
    }

    if total_size == 0 || !accept_ranges {
        // 不支持分块下载：单线程流式下载
        let _ = window.emit("download-file-progress", json!({ "stage": "单线程下载中...", "progress": 5 }));

        let get_req = client
            .get(&url)
            .header("Referer", &referer);
        let resp = get_req.send().await.map_err(|e| format!("下载失败: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("服务器返回错误: HTTP {}", resp.status()));
        }

        // 如果 HEAD 没拿到 size，用 GET 响应的 content-length
        let file_size = if total_size == 0 {
            resp.content_length().unwrap_or(0)
        } else {
            total_size
        };

        let mut file = fs::File::create(&dest).map_err(|e| e.to_string())?;
        let mut stream = resp.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last_emit_pct: i64 = -1;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("读取数据失败: {}", e))?;
            std::io::Write::write_all(&mut file, &chunk).map_err(|e| e.to_string())?;
            downloaded += chunk.len() as u64;

            if file_size > 0 {
                let pct = (downloaded * 100 / file_size) as i64;
                if pct != last_emit_pct {
                    last_emit_pct = pct;
                    let _ = window.emit("download-file-progress", json!({
                        "stage": format!("单线程下载中 ({}/{})", format_bytes(downloaded), format_bytes(file_size)),
                        "progress": 5 + (pct as u64 * 90 / 100),
                        "total": file_size,
                        "downloaded": chunk.len() as u64
                    }));
                }
            } else {
                // 未知大小：每 1MB 发射一次
                if downloaded % (1024 * 1024) < 256 * 1024 {
                    let _ = window.emit("download-file-progress", json!({
                        "stage": format!("单线程下载中 ({} 已下载)", format_bytes(downloaded)),
                        "downloaded": chunk.len() as u64
                    }));
                }
            }
        }

        let _ = window.emit("download-file-progress", json!({ "stage": "下载完成", "progress": 100 }));
        return Ok(save_path);
    }

    // 64 线程分块并发下载
    let chunk_size = 2 * 1024 * 1024u64; // 每块 2MB
    let num_chunks = ((total_size + chunk_size - 1) / chunk_size).max(1) as usize;
    let actual_threads = num_chunks.min(64);
    let sem = Arc::new(Semaphore::new(actual_threads));

    let _ = window.emit("download-file-progress", json!({
        "stage": format!("下载中 ({} 块, {} 总大小)...", num_chunks, format_bytes(total_size)),
        "progress": 0,
        "total": total_size,
        "downloaded": 0
    }));

    let temp_dir = dest.parent().unwrap_or(Path::new(".")).join(format!(
        ".lumia_dl_{}",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let chunk_paths: Vec<PathBuf> = (0..num_chunks)
        .map(|i| temp_dir.join(format!("chunk_{:04}", i)))
        .collect();

    let client = Arc::new(client);
    let mut tasks = Vec::new();

    let url = Arc::new(url);
    let referer = Arc::new(referer);

    for (i, chunk_path) in chunk_paths.iter().enumerate() {
        let start = i as u64 * chunk_size;
        let end = ((start + chunk_size - 1).min(total_size - 1));
        let client = client.clone();
        let sem = sem.clone();
        let url = url.clone();
        let referer = referer.clone();
        let window = window.clone();
        let chunk_path = chunk_path.clone();

        tasks.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            for retry in 0..3u8 {
                if retry > 0 {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                let resp = match client
                    .get(url.as_ref())
                    .header("Range", format!("bytes={}-{}", start, end))
                    .header("Referer", referer.as_str())
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT
                    && resp.status() != reqwest::StatusCode::OK
                {
                    continue;
                }
                match resp.bytes().await {
                    Ok(data) => {
                        if let Err(e) = fs::write(&chunk_path, &data) {
                            eprintln!("写入分块 {} 失败: {}", i, e);
                            return Err(format!("chunk {} write error", i));
                        }
                        let _ = window.emit("download-file-progress", json!({
                            "downloaded": data.len() as u64
                        }));
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("分块 {} 下载失败: {}", i, e);
                    }
                }
            }
            Err(format!("chunk {} failed", i))
        }));
    }

    // 等待所有分块完成
    for task in tasks {
        if let Err(e) = task.await.unwrap() {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(format!("分块下载失败: {}", e));
        }
    }

    // 合并分块
    let _ = window.emit("download-file-progress", json!({ "stage": "正在合并文件...", "progress": 95 }));
    let mut final_file = fs::File::create(&dest).map_err(|e| e.to_string())?;
    for chunk_path in &chunk_paths {
        let data = fs::read(chunk_path).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut final_file, &data).map_err(|e| e.to_string())?;
    }

    // 清理临时文件
    let _ = fs::remove_dir_all(&temp_dir);
    let _ = window.emit("download-file-progress", json!({ "stage": "下载完成", "progress": 100 }));

    Ok(save_path)
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// ========== 7.1 获取应用版本号 ==========
#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ========== 7.2 标记更新日志已读 ==========
#[tauri::command]
async fn mark_changelog_seen(app_handle: tauri::AppHandle, version: String) -> Result<(), String> {
    let state = app_handle.state::<AppState>();
    let mut config = state.config.lock().await;
    config.last_seen_version = version;
    drop(config); // 释放锁，避免 write 时死锁

    // 写回文件
    let cfg = state.config.lock().await;
    let json = serde_json::to_string_pretty(&*cfg).map_err(|e| e.to_string())?;
    fs::write(&state.config_path, json).map_err(|e| e.to_string())?;
    Ok(())
}

// ========== 7.3 自动检查更新（官方 API https://lumialauncher.cn/api.php?action=version） ==========

/// 与官方 API 同格式的当前版本标识（API 返回如 "v1.0 beta 1"）。
/// Cargo 版本 "1.0.0-beta.1" 与 "v1.0 beta 1" 是同一个版本，统一用它做比较基准。
const CURRENT_VERSION_LABEL: &str = "v1.0 beta 1";

/// 提取版本字符串中的数字序列："v1.0 beta 1" → [1,0,1]；"1.1.0" → [1,1,0]
fn version_num_vec(s: &str) -> Vec<i32> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect()
}

/// 逐位比较（缺位补 0），返回 a > b
fn version_greater(a: &[i32], b: &[i32]) -> bool {
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCheckResult {
    has_update: bool,
    current: String,
    latest: String,
    channel: String,
    beta: Option<String>,
    stable: Option<String>,
    beta_download: Option<String>,
    stable_download: Option<String>,
    /// API 新增：按平台返回的下载地址
    win_download: Option<String>,
    mac_download: Option<String>,
    /// 已按当前运行平台解析好的最终下载地址（前端直接用它）
    download_url: Option<String>,
}

/// 请求官方版本 API，比对当前版本，返回是否有新版本（失败时返回错误，前端静默忽略）
#[tauri::command]
async fn check_for_updates() -> Result<UpdateCheckResult, String> {
    let client = reqwest::Client::builder()
        .user_agent("Lumia-Launcher/1.0")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))?;

    let resp = client
        .get("https://lumialauncher.cn/api.php?action=version")
        .send()
        .await
        .map_err(|e| format!("检查更新请求失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("检查更新失败: HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析更新信息失败: {}", e))?;

    let v = &json["version"];
    let latest = v["latest"].as_str().unwrap_or("").to_string();
    let channel = v["channel"].as_str().unwrap_or("").to_string();
    let beta = v["beta"].as_str().map(|s| s.to_string());
    let stable = v["stable"].as_str().map(|s| s.to_string());
    let beta_download = v["beta_download"].as_str().map(|s| s.to_string());
    let stable_download = v["stable_download"].as_str().map(|s| s.to_string());

    // API 新增的平台专属字段（仅取非空值）
    let non_empty = |s: &Option<String>| s.clone().filter(|v| !v.is_empty());
    let win_download = non_empty(&v["win_download"].as_str().map(|s| s.to_string()));
    let mac_download = non_empty(&v["mac_download"].as_str().map(|s| s.to_string()));

    // 解析本次实际使用的下载地址：
    // 1) 当前平台的专属字段优先（避免 macOS 用户被导到 Windows 包）
    // 2) 回退到渠道字段（stable/beta）
    // 3) 最后兜底另一个渠道字段；beta_download 仍指向 Windows 包，兼容老版本
    let platform_download = if cfg!(target_os = "macos") {
        mac_download.clone()
    } else if cfg!(target_os = "windows") {
        win_download.clone()
    } else {
        None
    };
    let channel_download = if channel == "stable" {
        stable_download.clone()
    } else {
        beta_download.clone()
    };
    let download_url = non_empty(&platform_download)
        .or_else(|| non_empty(&channel_download))
        .or_else(|| non_empty(&beta_download))
        .or_else(|| non_empty(&stable_download));

    let current = CURRENT_VERSION_LABEL.to_string();
    let latest_nums = version_num_vec(&latest);
    let current_nums = version_num_vec(&current);
    let has_update = !latest.is_empty() && version_greater(&latest_nums, &current_nums);

    Ok(UpdateCheckResult {
        has_update,
        current,
        latest,
        channel,
        beta,
        stable,
        beta_download,
        stable_download,
        win_download,
        mac_download,
        download_url,
    })
}

// ========== 8. 扫描 Java 路径 ==========
#[tauri::command]
async fn get_java_paths() -> Result<Vec<String>, String> {
    // 与启动探测同一套逻辑：候选目录递归搜索 + 环境变量（含 D 盘等任意位置）
    let mut paths: Vec<String> = Vec::new();

    for root in candidate_java_roots() {
        let is_drive_root = cfg!(windows) && root.as_os_str().to_string_lossy().ends_with('\\');
        let max_depth = if is_drive_root { 2 } else { 4 };
        let mut found: Vec<String> = Vec::new();
        find_java_recursive(&root, &mut found, 0, max_depth);
        for p in found {
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    }

    for var in ["JAVA_HOME", "JDK_HOME"] {
        if let Ok(home) = std::env::var(var) {
            for part in home.split(';') {
                let java_exe = PathBuf::from(part.trim().trim_matches('"'))
                    .join("bin")
                    .join(java_exe_name());
                if java_exe.exists() {
                    let p = java_exe.to_str().unwrap().to_string();
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
            }
        }
    }

    // 版本高的优先
    paths.sort_by(|a, b| version_from_path(b).cmp(&version_from_path(a)));
    Ok(paths)
}

#[tauri::command]
async fn get_required_java(version: String) -> Result<u32, String> {
    let v = version.clone();
    if v.starts_with("1.16") || v.starts_with("1.15") || v.starts_with("1.14") {
        return Ok(8);
    } else if v.starts_with("1.17") {
        return Ok(16);
    } else if v.starts_with("1.18") || v.starts_with("1.19") || v.starts_with("1.20") {
        return Ok(17);
    } else if v.starts_with("1.21") || v.starts_with("1.22") {
        return Ok(21);
    } else if v.starts_with("26") {
        return Ok(25);
    }
    Ok(17)
}

// ========== 10. 启动设备代码认证 ==========
#[tauri::command]
async fn open_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("打开网页失败: {}", e))
}

#[tauri::command]
async fn start_device_auth(state: tauri::State<'_, AppState>) -> Result<auth::AuthResult, String> {
    let client = reqwest::Client::builder()
        .user_agent("Lumia-Launcher/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))?;

    let device = auth::request_device_code(&client).await?;

    // 存储设备代码状态
    let mut dc = state.device_code.lock().await;
    *dc = Some(DeviceCodeState {
        device_code: device.device_code.clone(),
        interval: device.interval,
        expires_at: std::time::Instant::now() + std::time::Duration::from_secs(device.expires_in),
    });

    // 打开浏览器
    let _ = open::that(&device.verification_uri);

    Ok(auth::AuthResult {
        success: false,
        uuid: None,
        username: None,
        access_token: None,
        error: None,
        user_code: Some(device.user_code),
        verification_uri: Some(device.verification_uri),
        message: Some("请在浏览器中输入代码".to_string()),
    })
}

// ========== 11. 轮询设备认证 ==========
#[tauri::command]
async fn poll_device_auth(
    state: tauri::State<'_, AppState>,
    _app_handle: tauri::AppHandle,
) -> Result<auth::AuthResult, String> {
    let device_code = {
        let dc = state.device_code.lock().await;
        let dc = dc.as_ref().ok_or("没有进行中的认证流程")?;
        if std::time::Instant::now() > dc.expires_at {
            return Err("设备代码已过期，请重新登录".to_string());
        }
        dc.device_code.clone()
    };

    let client = reqwest::Client::builder()
        .user_agent("Lumia-Launcher/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))?;

    match auth::poll_for_token(&client, &device_code).await {
        Ok(token) => {
            let ms_token = token.access_token.clone();
            let refresh = token.refresh_token.clone();

            match auth::ms_token_to_mc_profile(&client, &ms_token).await {
                Ok((profile, mc_auth)) => {
                    // 保存到配置
                    let mut config = state.config.lock().await;
                    config.ms_refresh_token = refresh.or_else(|| config.ms_refresh_token.clone());
                    config.ms_access_token = Some(mc_auth.access_token);
                    config.ms_uuid = Some(profile.id.clone());
                    config.ms_username = Some(profile.name.clone());
                    config.username = profile.name.clone();

                    let json = serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?;
                    fs::write(&state.config_path, json).map_err(|e| e.to_string())?;

                    // 清除设备代码
                    let mut dc = state.device_code.lock().await;
                    *dc = None;

                    Ok(auth::AuthResult {
                        success: true,
                        uuid: Some(profile.id),
                        username: Some(profile.name),
                        access_token: config.ms_access_token.clone(),
                        error: None,
                        user_code: None,
                        verification_uri: None,
                        message: Some("登录成功".to_string()),
                    })
                }
                Err(e) => {
                    let mut dc = state.device_code.lock().await;
                    *dc = None;
                    Err(e)
                }
            }
        }
        Err(e) => {
            if e == "authorization_pending" || e == "slow_down" {
                Ok(auth::AuthResult {
                    success: false,
                    uuid: None,
                    username: None,
                    access_token: None,
                    error: None,
                    user_code: None,
                    verification_uri: None,
                    message: Some("等待用户授权...".to_string()),
                })
            } else {
                let mut dc = state.device_code.lock().await;
                *dc = None;
                Err(e)
            }
        }
    }
}

// ========== 12. 检查认证状态 ==========
#[tauri::command]
async fn check_auth_status(state: tauri::State<'_, AppState>) -> Result<auth::AuthResult, String> {
    let (refresh, name, uuid, cached_token) = {
        let config = state.config.lock().await;
        (
            config.ms_refresh_token.clone(),
            config.ms_username.clone(),
            config.ms_uuid.clone(),
            config.ms_access_token.clone(),
        )
    };

    match (refresh, name, uuid) {
        (Some(refresh), Some(name), Some(uuid)) => {
            let client = reqwest::Client::builder()
                .user_agent("Lumia-Launcher/0.1")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|e| format!("创建网络客户端失败: {}", e))?;

            match auth::refresh_access_token(&client, &refresh).await {
                Ok(token) => {
                    match auth::ms_token_to_mc_profile(&client, &token.access_token).await {
                        Ok((profile, mc_auth)) => {
                            let mut config = state.config.lock().await;
                            config.ms_refresh_token = token.refresh_token.or(Some(refresh));
                            config.ms_access_token = Some(mc_auth.access_token);
                            config.ms_uuid = Some(profile.id.clone());
                            config.ms_username = Some(profile.name.clone());
                            config.username = profile.name.clone();

                            let json = serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?;
                            fs::write(&state.config_path, json).map_err(|e| e.to_string())?;

                            Ok(auth::AuthResult {
                                success: true,
                                uuid: Some(profile.id),
                                username: Some(profile.name),
                                access_token: config.ms_access_token.clone(),
                                error: None,
                                user_code: None,
                                verification_uri: None,
                                message: None,
                            })
                        }
                        Err(e) => Ok(auth::AuthResult {
                            success: true,
                            uuid: Some(uuid.clone()),
                            username: Some(name.clone()),
                            access_token: cached_token.clone(),
                            error: None,
                            user_code: None,
                            verification_uri: None,
                            message: Some(format!("Token 刷新成功但获取玩家信息失败: {}", e)),
                        }),
                    }
                }
                Err(_) => Ok(auth::AuthResult {
                    success: true,
                    uuid: Some(uuid.clone()),
                    username: Some(name.clone()),
                    access_token: cached_token.clone(),
                    error: None,
                    user_code: None,
                    verification_uri: None,
                    message: Some("Token 可能已过期，请重新登录".to_string()),
                }),
            }
        }
        _ => Ok(auth::AuthResult {
            success: false,
            uuid: None,
            username: None,
            access_token: None,
            error: None,
            user_code: None,
            verification_uri: None,
            message: Some("未登录".to_string()),
        }),
    }
}

// ========== 13. 退出登录 ==========
#[tauri::command]
async fn logout(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut config = state.config.lock().await;
    config.ms_refresh_token = None;
    config.ms_access_token = None;
    config.ms_uuid = None;
    config.ms_username = None;

    let json = serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?;
    fs::write(&state.config_path, json).map_err(|e| e.to_string())?;

    Ok(())
}

// ========== Touch Bar 进度更新（macOS；其他平台为空操作） ==========
#[tauri::command]
async fn touchbar_set_progress(app: tauri::AppHandle, progress: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    touchbar::set_progress(&app, progress);
    // 非 macOS：Touch Bar 不存在，参数不参与逻辑（用下划线消费避免 unused 告警）
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (&app, progress);
    }
    Ok(())
}

#[tauri::command]
async fn touchbar_update_active_items(app: tauri::AppHandle, active: Vec<String>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    touchbar::update_active_items(&app, &active);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (&app, active);
    }
    Ok(())
}

// ========== 主函数 ==========
fn main() {
    // Lumia 专属游戏目录（mac/win/linux 各自独立，不与任何其他启动器共用）
    let game_dir = default_game_dir();

    // 一次性迁移：旧版曾使用 ~/.minecraft，首次运行新目录时把旧数据整体搬过去，
    // 避免用户重新下载几 GB 的版本/库/资源（官方启动器的目录是另一个路径，不受影响）
    if !game_dir.exists() {
        let home = std::env::var("HOME").unwrap_or_else(|_| String::new());
        let old_dir = PathBuf::from(&home).join(".minecraft");
        if old_dir.exists() {
            if let Some(parent) = game_dir.parent() {
                fs::create_dir_all(parent).unwrap_or_default();
            }
            match fs::rename(&old_dir, &game_dir) {
                Ok(_) => eprintln!("已迁移游戏目录: {:?} → {:?}", old_dir, game_dir),
                Err(e) => eprintln!("迁移游戏目录失败（保留原目录，使用新目录）: {}", e),
            }
        }
    }
    fs::create_dir_all(&game_dir).unwrap_or_default();
    let config_path = game_dir.join("lumia_config.json");

    let config = if config_path.exists() {
        fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        AppConfig::default()
    };

    let state = AppState {
        config: Arc::new(Mutex::new(config)),
        config_path,
        device_code: Arc::new(Mutex::new(None)),
        cancel_download: Arc::new(AtomicBool::new(false)),
    };

    tauri::Builder::default()
        .manage(state)
        .manage(plugins::PluginRegistryState(Arc::new(std::sync::Mutex::new(plugins::PluginRegistry::new()))))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("stream", |ctx, request, responder| {
            // 本地音乐库流式播放协议：stream://localhost/<歌单>/<文件>
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            let query = request.uri().query().unwrap_or("").to_string();
            // 把 Range 头传给处理器：<audio> 分段拉取/拖动进度靠它，缺了会反复整文件重载 → 断续
            let range = request
                .headers()
                .get(tauri::http::header::RANGE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            tauri::async_runtime::spawn(async move {
                let full = if query.is_empty() { path } else { format!("{path}?{query}") };
                let resp = music::handle_stream_request(&app, &full, range.as_deref()).await;
                responder.respond(resp);
            });
        })
        .setup(|app| {
            let config = app.state::<AppState>().config.clone();
            let config_path = app.state::<AppState>().config_path.clone();
            tauri::async_runtime::spawn(async move {
                let refresh = {
                    let cfg = config.lock().await;
                    cfg.ms_refresh_token.clone()
                };
                if let Some(refresh) = refresh {
                    let client = match reqwest::Client::builder()
                        .user_agent("Lumia-Launcher/0.1")
                        .timeout(std::time::Duration::from_secs(15))
                        .build()
                    {
                        Ok(c) => c,
                        Err(_) => return,
                    };
                    if let Ok(token) = auth::refresh_access_token(&client, &refresh).await {
                        if let Ok((profile, mc_auth)) = auth::ms_token_to_mc_profile(&client, &token.access_token).await {
                            let mut cfg = config.lock().await;
                            cfg.ms_refresh_token = token.refresh_token.or(Some(refresh));
                            cfg.ms_access_token = Some(mc_auth.access_token);
                            cfg.ms_uuid = Some(profile.id);
                            cfg.ms_username = Some(profile.name.clone());
                            cfg.username = profile.name;
                            if let Ok(json) = serde_json::to_string_pretty(&*cfg) {
                                let _ = fs::write(&config_path, json);
                            }
                        }
                    }
                }
            });
            // macOS Touch Bar 初始化（在 setup 主线程执行）
            #[cfg(target_os = "macos")]
            if let Err(e) = touchbar::setup(app.handle()) {
                eprintln!("Touch Bar 初始化失败: {}", e);
            }

            // 注意：dev 模式下不设置 Dock 图标，保留系统默认的 "exec" 图标样式；
            // 正式打包时 Dock 图标由 bundle.icon 中的资源生成，无需运行时干预。
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            touchbar_set_progress,
            touchbar_update_active_items,
            get_versions,
            get_local_versions,
            download_game,
            launch_game,
            open_folder,
            open_crash_log_file,
            parse_crash_report,
            get_fabric_versions,
            get_forge_versions,
            get_neoforge_versions,
            get_config,
            save_config,
            get_app_version,
            mark_changelog_seen,
            check_for_updates,
            get_java_paths,
            get_required_java,
            start_device_auth,
            poll_device_auth,
            check_auth_status,
            logout,
            cancel_download,
            search_mods,
            check_mod_support,
            install_mod,
            search_resource_packs,
            install_resource_pack,
            search_shader_packs,
            install_shader_pack,
            open_url,
            rename_version,
            delete_version,
            list_mods,
            toggle_mod,
            delete_mod,
            open_mods_folder,
            get_offline_uuid,
            inspect_modpack,
            install_modpack,
            terracotta_start,
            terracotta_status,
            terracotta_host,
            terracotta_join,
            terracotta_stop,
            terracotta_back,
            is_game_running,
            terracotta_nodes_status,
            offline_mode_allowed,
            ai_chat,
            ai_execute_file_ops,
            list_ai_models,
            music::music_import_paths,
            music::music_import_folder,
            music::music_list,
            music::music_delete_playlist,
            music::music_remove_song,
            plugins::plugin_list,
            plugins::plugin_ui_tree,
            plugins::plugin_page_injects,
            plugins::plugin_ui_event,
            plugins::plugin_emit_event,
            plugins::plugin_set_enabled,
            plugins::plugin_delete,
            plugins::plugin_open_dir,
            save_background_image,
            remove_background_image,
            download_file,
        ])
        .run(tauri::generate_context!())
        .expect("启动应用失败");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 构造隔离的临时 HOME，避免污染真实 ~/terracotta
    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumia-tc-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_log(home: &Path, dir_name: &str, content: &str) {
        let d = home.join("terracotta").join(dir_name);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("application.log"), content).unwrap();
    }

    #[test]
    fn terracotta_log_helpers() {
        let home = temp_home("tc");
        // 先写"旧"目录，睡 1.2s 再写"新"目录，保证目录 mtime 可区分
        write_log(
            &home,
            "2026-08-01-00-00-00-11111",
            "[State]: Switch to AppState::HostStarting { code: \"U/AAAA-BBBB-CCCC-DDDD\", port: 1 }\n",
        );
        std::thread::sleep(std::time::Duration::from_millis(1200));
        write_log(
            &home,
            "2026-08-02-00-00-00-22222",
            "2026-08-02 00:00:01: [x] connect to peer error. dst: https://etnode.zkitefly.eu.org/node1, err: Connection reset by peer\n",
        );

        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &home);

        // 1. 目录从新到旧
        let dirs = terracotta_log_dirs();
        assert_eq!(dirs.len(), 2);
        assert!(dirs[0].0.ends_with("22222"), "最新的应排最前: {:?}", dirs[0].0);
        assert!(dirs[1].0.ends_with("11111"));

        // 2. 房间码从含 code 的日志解析（此处是旧目录）
        assert_eq!(terracotta_last_room_code().as_deref(), Some("U/AAAA-BBBB-CCCC-DDDD"));

        // 3. 最新日志里 node1 握手失败 → 判定节点不可用；没出现过的 URL 不误判
        assert!(terracotta_log_node_failed("https://etnode.zkitefly.eu.org/node1"));
        assert!(!terracotta_log_node_failed("tcp://custom-node:11010"));

        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn old_config_without_language_still_loads() {
        // 旧版配置文件没有 language 字段：serde(default) 应回退到 "auto"，不能反序列化失败
        let old_json = r#"{
            "username": "Player",
            "max_memory": 2048,
            "use_rosetta": false
        }"#;
        let cfg: AppConfig = serde_json::from_str(old_json).expect("旧配置文件必须能加载");
        assert_eq!(cfg.language, "auto");
    }

    #[test]
    fn trace_body_loc_parsing() {
        // Cloudflare trace 响应：loc=CN 判定大陆，其他地区/无 loc 判定非大陆
        let cn_body = "fl=123\nh=www.cloudflare.com\nip=1.2.3.4\nloc=CN\ncolo=SHA\n";
        assert!(trace_body_is_cn(cn_body));
        let us_body = "fl=1\nh=www.cloudflare.com\nip=1.2.3.4\nloc=US\ncolo=LAX\n";
        assert!(!trace_body_is_cn(us_body));
        assert!(!trace_body_is_cn("fl=1\nh=x\nip=1.2.3.4\n"));
        assert!(!trace_body_is_cn(""));
    }

    #[test]
    fn ai_file_ops_sandbox() {
        // AI 文件操作必须限制在游戏目录内：写/删可执行，../ 穿越必须被拒绝
        let base = std::env::temp_dir().join(format!("lumia-ai-op-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("mods")).unwrap();
        let base_canon = base.canonicalize().unwrap();

        // 1. 正常写入 + 删除
        let write_op = AiFileOp { action: "write_file".into(), path: "mods/ai-test.jar".into(), content: Some("data".into()) };
        let r = apply_ai_file_op(&base_canon, &write_op).unwrap();
        assert_eq!(r["ok"], true);
        assert!(base_canon.join("mods/ai-test.jar").is_file());

        let del_op = AiFileOp { action: "delete_file".into(), path: "mods/ai-test.jar".into(), content: None };
        let r = apply_ai_file_op(&base_canon, &del_op).unwrap();
        assert_eq!(r["ok"], true);
        assert!(!base_canon.join("mods/ai-test.jar").exists());

        // 2. 穿越（../ 逃出游戏目录）必须被拒绝
        let escape_op = AiFileOp { action: "write_file".into(), path: "../escaped.txt".into(), content: Some("evil".into()) };
        assert!(apply_ai_file_op(&base_canon, &escape_op).is_err());

        // 3. 父目录不存在 → 拒绝（不自动建目录）
        let missing_op = AiFileOp { action: "write_file".into(), path: "no-such-dir/x.txt".into(), content: Some("x".into()) };
        assert!(apply_ai_file_op(&base_canon, &missing_op).is_err());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn mod_toggle_and_delete_roundtrip() {
        // 模拟版本管理：启用/禁用（.jar ↔ .jar.disabled）与删除
        let mods_dir = std::env::temp_dir().join(format!("lumia-mods-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&mods_dir);
        fs::create_dir_all(&mods_dir).unwrap();

        // 写入一个假模组
        fs::write(mods_dir.join("MyMod.jar"), b"jar").unwrap();

        // 禁用
        toggle_mod_file(&mods_dir, "MyMod.jar", false).unwrap();
        assert!(!mods_dir.join("MyMod.jar").exists());
        assert!(mods_dir.join("MyMod.jar.disabled").exists());

        // 重新启用（fileName 传禁用态的名字也能正确恢复）
        toggle_mod_file(&mods_dir, "MyMod.jar.disabled", true).unwrap();
        assert!(mods_dir.join("MyMod.jar").exists());
        assert!(!mods_dir.join("MyMod.jar.disabled").exists());

        // 删除（传启用态名字，能同时清理两类文件）
        fs::write(mods_dir.join("MyMod.jar.disabled"), b"jar").unwrap();
        delete_mod_file(&mods_dir, "MyMod.jar").unwrap();
        assert!(!mods_dir.join("MyMod.jar").exists());
        assert!(!mods_dir.join("MyMod.jar.disabled").exists());

        let _ = fs::remove_dir_all(&mods_dir);
    }

    #[test]
    fn ai_read_tools_sandbox() {
        // AI 只读工具必须限制在游戏目录内
        let base = std::env::temp_dir().join(format!("lumia-ai-read-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("logs")).unwrap();
        fs::write(base.join("logs/latest.log"), "line1\nline2\n".repeat(100)).unwrap();
        fs::write(base.join("binary.bin"), vec![0u8, 1, 2, 0]).unwrap();
        let base_canon = base.canonicalize().unwrap();

        // 1. 列出根目录与子目录
        let out = exec_list_dir(&base_canon, &serde_json::json!({ "path": "" })).unwrap();
        assert!(out.contains("logs"));
        assert!(out.contains("binary.bin"));
        let out = exec_list_dir(&base_canon, &serde_json::json!({ "path": "logs" })).unwrap();
        assert!(out.contains("latest.log"));

        // 2. 读取文本（截断生效）
        let out = exec_read_file(&base_canon, &serde_json::json!({ "path": "logs/latest.log", "max_chars": 20 })).unwrap();
        assert!(out.chars().count() <= 20);

        // 3. 二进制跳过
        let out = exec_read_file(&base_canon, &serde_json::json!({ "path": "binary.bin" })).unwrap();
        assert!(out.contains("二进制") || out.contains("binary"));

        // 4. 穿越/不存在 → 拒绝
        assert!(exec_read_file(&base_canon, &serde_json::json!({ "path": "../secret.txt" })).is_err());
        assert!(exec_read_file(&base_canon, &serde_json::json!({ "path": "nope.txt" })).is_err());

        let _ = fs::remove_dir_all(&base);
    }
}
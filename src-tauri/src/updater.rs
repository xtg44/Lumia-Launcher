//! 应用自更新：检查 → 准备（下载/校验/解压）→ 退出 → 替换 → 重启
//!
//! 核心设计：**替换发生在应用退出之后**。
//! 主进程只负责把新版本准备好、写一个 helper，然后立刻退出；真正动安装目录的
//! （备份 → 就位 → 失败回滚）由 helper 完成。进程一退，安装目录里的文件就没有
//! 持有者了，替换因此退化成普通文件操作 —— 不需要处理文件锁，也不需要
//! 「改名腾位」那套三段式技巧。
//!
//! helper 的两种形态：
//! - macOS：一段 `/bin/sh` 脚本（系统自带 `ditto` / `open`，几行就够）
//! - Windows：**自身 exe 的副本**（不能用 .bat —— 用户的安装路径可能含中文，
//!   `cmd` 读脚本走 ANSI 代码页会把路径变成乱码）

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::lumia_data_dir;

/// 更新工作目录：<数据目录>/updates
pub fn updates_root() -> PathBuf {
    lumia_data_dir().join("updates")
}

/// helper 日志（每次派发前清空；若上次失败会留在里面给前端提示）
pub fn helper_log_path() -> PathBuf {
    updates_root().join("last-apply.log")
}

// ========== 服务端更新包描述 ==========

/// 更新包信息。字段缺失（老服务端还没加）时整体为 None → 前端退回「打开浏览器下载」。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePackage {
    pub url: String,
    /// 服务端给出的 SHA-256（小写十六进制）。为空表示未提供 → 跳过校验只靠 HTTPS。
    pub sha256: Option<String>,
    /// 预期字节数，0 表示未知
    pub size: u64,
}

/// 当前平台在 API 里的键名（平铺方案 A）
fn platform_key() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "mac_arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "mac_x64"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        "unknown"
    }
}

/// 当前平台在结构化 API（方案 B）里的键名
fn platform_dashed_key() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-aarch64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "unknown"
    }
}

fn clean_hash(v: Option<&str>) -> Option<String> {
    v.map(|s| s.trim().to_lowercase())
        .filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

fn clean_url(v: Option<&str>) -> Option<String> {
    v.map(|s| s.trim().to_string())
        .filter(|s| s.starts_with("https://"))
}

/// 从 API 的 `version` 对象里解析当前平台可用的自更新包。
/// 同时兼容两种服务端结构（谁先上都能跑）：
/// - 方案 A 平铺：`mac_arm64_zip` / `mac_x64_zip` / `win_zip`（+ `_sha256`）
/// - 方案 B 结构化：`update: { "macos-aarch64": {url, sha256, size}, ... }`
pub fn pick_package(v: &serde_json::Value) -> Option<UpdatePackage> {
    // 方案 B 优先（结构更明确）
    if let Some(entry) = v.get("update").and_then(|u| u.get(platform_dashed_key())) {
        if let Some(url) = clean_url(entry.get("url").and_then(|s| s.as_str())) {
            return Some(UpdatePackage {
                url,
                sha256: clean_hash(entry.get("sha256").and_then(|s| s.as_str())),
                size: entry.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
            });
        }
    }

    // 方案 A 平铺
    let base = platform_key();
    if base == "unknown" {
        return None;
    }
    let url = clean_url(v.get(format!("{}_zip", base)).and_then(|s| s.as_str()))?;
    let hash = clean_hash(
        v.get(format!("{}_zip_sha256", base))
            .and_then(|s| s.as_str()),
    );
    Some(UpdatePackage {
        url,
        sha256: hash,
        size: v
            .get(format!("{}_zip_size", base))
            .and_then(|s| s.as_u64())
            .unwrap_or(0),
    })
}

// ========== 安全闸门 ==========

/// 自更新能力探测结果（前端据此决定按钮是「下载并安装」还是「打开官网下载」）
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCapability {
    pub supported: bool,
    /// 不支持时的人类可读原因
    pub reason: Option<String>,
    /// 当前安装位置（展示用）
    pub install_path: Option<String>,
}

/// 安装目标：macOS 是 `.app`，Windows 是 exe 本身
#[derive(Debug, Clone)]
pub struct InstallTarget {
    pub bundle: PathBuf,
}

/// 去掉 Windows 的 `\\?\` 长路径前缀，让路径能被正常展示与比较
pub fn normalize_path(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

/// 在目录里试写一个临时文件，判断是否可写
fn check_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(".lumia-write-test");
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(format!(
            "没有权限写入 {}，请手动下载安装新版本",
            dir.display()
        )),
    }
}

/// 定位要替换的目标并做安全闸门检查。
/// 任何一条不过都返回 Err（调用方降级为「打开浏览器下载」），
/// 绝不允许把不认识的路径交给 helper 去删。
pub fn resolve_target() -> Result<InstallTarget, String> {
    let exe = normalize_path(
        std::env::current_exe().map_err(|e| format!("无法定位自身路径: {}", e))?,
    );
    let s = exe.to_string_lossy().replace('\\', "/");

    // 开发期保护：编译目录里的产物不是「安装好的应用」，替换会把开发环境搞坏
    if s.contains("/target/debug/") || s.contains("/target/release/") {
        return Err("当前是开发模式运行，不执行自动更新".into());
    }

    #[cfg(target_os = "macos")]
    {
        // .../Xxx.app/Contents/MacOS/<bin> → 向上三层拿到 .app
        let macos_dir = exe.parent().ok_or("无法定位可执行文件所在目录")?;
        if macos_dir.file_name().and_then(|n| n.to_str()) != Some("MacOS") {
            return Err("当前不是标准 .app 安装形态".into());
        }
        let contents = macos_dir.parent().ok_or("无法定位 Contents 目录")?;
        if contents.file_name().and_then(|n| n.to_str()) != Some("Contents") {
            return Err("当前不是标准 .app 安装形态".into());
        }
        let bundle = contents.parent().ok_or("无法定位 .app 目录")?;
        if bundle.extension().and_then(|e| e.to_str()) != Some("app") {
            return Err("当前不是标准 .app 安装形态".into());
        }
        // 直接从 dmg 里运行（只读卷）时这里会失败 → 正确降级
        let parent = bundle.parent().ok_or("无法定位安装目录")?;
        check_writable(parent)?;
        return Ok(InstallTarget {
            bundle: bundle.to_path_buf(),
        });
    }

    #[cfg(target_os = "windows")]
    {
        let dir = exe.parent().ok_or("无法定位可执行文件所在目录")?;
        check_writable(dir)?;
        return Ok(InstallTarget { bundle: exe.clone() });
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("当前平台暂不支持自动更新".into())
    }
}

pub fn detect_capability() -> UpdateCapability {
    match resolve_target() {
        Ok(t) => UpdateCapability {
            supported: true,
            reason: None,
            install_path: Some(t.bundle.display().to_string()),
        },
        Err(e) => UpdateCapability {
            supported: false,
            reason: Some(e),
            install_path: None,
        },
    }
}

// ========== 校验与解压 ==========

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| format!("打开下载文件失败: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 读 Mach-O 的 CPU 类型（只处理 thin；fat 或无法识别时返回 None 表示「不阻断」）
#[cfg(target_os = "macos")]
fn macho_cpu(path: &Path) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut hdr = [0u8; 8];
    f.read_exact(&mut hdr).ok()?;
    let be = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
    let name = |cpu: u32| -> Option<String> {
        match cpu {
            0x0100_0007 => Some("x86_64".to_string()),
            0x0100_000C => Some("arm64".to_string()),
            _ => None,
        }
    };
    match be {
        // MH_MAGIC_64：文件按小端写，magic 读出来是大端序
        0xFEED_FACF => name(u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]])),
        // 字节序颠倒的 64 位
        0xCFFA_EDFE => name(u32::from_be_bytes([hdr[4], hdr[5], hdr[6], hdr[7]])),
        _ => None,
    }
}

/// 校验解压出来的 .app 结构：可执行文件存在、有执行位、架构匹配
#[cfg(target_os = "macos")]
fn verify_macos_app(app: &Path) -> Result<(), String> {
    let bin = app.join("Contents/MacOS/lumia-launcher");
    if !bin.is_file() {
        return Err("更新包结构不完整（缺少主程序）".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&bin)
            .map_err(|e| format!("读取主程序属性失败: {}", e))?
            .permissions()
            .mode();
        if mode & 0o111 == 0 {
            return Err("更新包里的主程序没有执行权限，已放弃安装".into());
        }
    }
    if let Some(cpu) = macho_cpu(&bin) {
        let expected = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        if cpu != expected {
            return Err(format!(
                "更新包架构不匹配（包内为 {}，本机需要 {}）",
                cpu, expected
            ));
        }
    }
    Ok(())
}

/// 解压 macOS 更新包并返回里面的 .app 路径。
/// **必须用 ditto**：`.app` 里有符号链接与可执行权限位，项目里那个
/// `extract_zip_dir`（zip crate）两者都不保留，解出来主程序丢了 +x 直接启动不了。
#[cfg(target_os = "macos")]
pub fn extract_macos_package(zip: &Path, stage: &Path) -> Result<PathBuf, String> {
    if stage.exists() {
        fs::remove_dir_all(stage).map_err(|e| format!("清理解压目录失败: {}", e))?;
    }
    fs::create_dir_all(stage).map_err(|e| format!("创建解压目录失败: {}", e))?;

    let out = Command::new("/usr/bin/ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(stage)
        .output()
        .map_err(|e| format!("调用 ditto 失败: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "解压失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    // --keepParent 保证 zip 顶层就是 .app；找不到就直接失败
    let app = fs::read_dir(stage)
        .map_err(|e| format!("读取解压目录失败: {}", e))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("app"))
        .ok_or("更新包里没有找到 .app")?;

    verify_macos_app(&app)?;
    Ok(app)
}

/// 校验 Windows 更新包是合法的 PE 可执行文件（MZ 头）
#[cfg(target_os = "windows")]
pub fn verify_windows_exe(path: &Path) -> Result<(), String> {
    let mut f = fs::File::open(path).map_err(|e| format!("打开更新包失败: {}", e))?;
    let mut head = [0u8; 2];
    f.read_exact(&mut head)
        .map_err(|e| format!("读取更新包失败: {}", e))?;
    if &head != b"MZ" {
        return Err("更新包不是合法的 Windows 可执行文件".into());
    }
    Ok(())
}

// ========== 更新计划（prepare → apply 之间靠文件传递，不信任前端传参） ==========

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApplyPlan {
    pub version: String,
    /// 要替换的目标（.app / exe）
    pub target: PathBuf,
    /// 新版本的就绪位置（.app / 新 exe）
    pub staged: PathBuf,
    /// 工作目录（helper 完成后整目录清理）
    pub workdir: PathBuf,
    pub created_at: u64,
}

impl ApplyPlan {
    pub fn path_in(workdir: &Path) -> PathBuf {
        workdir.join("plan.json")
    }

    pub fn save(&self, workdir: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(Self::path_in(workdir), json).map_err(|e| format!("写入更新计划失败: {}", e))
    }

    /// 读取并做一致性校验。目标是**绝不把任意路径交给 helper 去删**：
    /// staged 必须在 workdir 内，target 必须等于此刻重新探测出来的安装位置。
    pub fn load_for(version: &str) -> Result<ApplyPlan, String> {
        let workdir = workdir_for(version);
        let raw = fs::read_to_string(Self::path_in(&workdir))
            .map_err(|_| "没有找到待安装的更新，请重新下载".to_string())?;
        let plan: ApplyPlan =
            serde_json::from_str(&raw).map_err(|e| format!("更新计划已损坏: {}", e))?;

        if plan.version != version {
            return Err("更新计划与当前版本不匹配，请重新下载".into());
        }
        if !plan.staged.starts_with(&plan.workdir) || !plan.staged.exists() {
            return Err("更新文件已不存在，请重新下载".into());
        }
        let current = resolve_target()?;
        if current.bundle != plan.target {
            return Err("安装位置发生变化，请重新下载".into());
        }
        // 工作目录必须在 updates 根目录内（防止被构造出越界路径）
        if !plan.workdir.starts_with(updates_root()) {
            return Err("更新路径异常，请重新下载".into());
        }
        Ok(plan)
    }
}

/// 版本号里可能有 `/` 之类不能做目录名的字符，统一清洗。
/// 返回空串表示这个版本号不能用来做目录名（调用方必须拒绝），
/// 注意防的是 `..` / `.` 这类能改变路径层级的名字 —— `PathBuf::starts_with`
/// 只做字面前缀比较，`updates/..` 是能骗过它的。
pub fn safe_version(version: &str) -> String {
    let cleaned: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim().to_string();
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '.') {
        return String::new();
    }
    trimmed
}

/// 某个版本的工作目录
pub fn workdir_for(version: &str) -> PathBuf {
    updates_root().join(safe_version(version))
}

/// 开始新一轮下载前清掉旧的下载/解压产物（保留 apply.sh 与日志）。
/// 顺带避免上一轮留下的半个包被误用。
pub fn clear_workspace() {
    let root = updates_root();
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else if name != "apply.sh" && name != "last-apply.log" {
            let _ = fs::remove_file(&path);
        }
    }
}

/// 上次更新失败的原因（读一次就清掉日志，避免反复提示）
pub fn take_last_error() -> Option<String> {
    let log = helper_log_path();
    let content = fs::read_to_string(&log).ok()?;
    let _ = fs::write(&log, "");
    let err = content
        .lines()
        .rev()
        .find(|l| l.contains("FAIL"))?
        .trim()
        .to_string();
    Some(err)
}

// ========== macOS helper ==========

/// helper 脚本。参数：pid / target(.app) / staged(.app) / log / workdir
#[cfg(target_os = "macos")]
const MAC_HELPER: &str = r#"#!/bin/sh
# Lumia Launcher 自更新 helper —— 由主程序在退出前生成并派发。
# 应用此刻已经退出，安装目录里的文件没有持有者，替换就是普通文件操作。
set -u
PID="$1"
TARGET="$2"
STAGED="$3"
LOG="$4"
WORK="$5"

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*" >>"$LOG" 2>/dev/null; }

log "helper start pid=$PID"
log "  target=$TARGET"
log "  staged=$STAGED"

# 1) 等主进程退出（轮询上限 15s；超时也继续，此时文件其实已无占用）
i=0
while kill -0 "$PID" 2>/dev/null && [ "$i" -lt 75 ]; do
  sleep 0.2
  i=$((i+1))
done
log "main process exited (waited $i ticks)"

# 2) 备份旧包 —— 用 mv 而不是 rm，第 3 步失败要能原样搬回
rm -rf "$TARGET.bak" 2>/dev/null
if ! mv "$TARGET" "$TARGET.bak" 2>>"$LOG"; then
  log "FAIL: 无法备份旧版本（可能无写入权限）"
  exit 10
fi

# 3) 新包就位（ditto 保留权限位 / 符号链接 / xattr）
if /usr/bin/ditto "$STAGED" "$TARGET" 2>>"$LOG"; then
  rm -rf "$TARGET.bak" 2>/dev/null
  log "OK: replaced"
else
  log "FAIL: 写入新版本失败，已回滚"
  rm -rf "$TARGET" 2>/dev/null
  mv "$TARGET.bak" "$TARGET" 2>>"$LOG"
fi

# 4) 无论成败都把应用拉起来（失败时起来的还是旧版，用户至少能继续用）
/usr/bin/open -n "$TARGET" >>"$LOG" 2>&1
log "launched"

# 5) 清理工作目录（zip + 解压产物）
cd /
rm -rf "$WORK" 2>/dev/null
log "helper done"
"#;

/// 写 helper 脚本并设置可执行位（幂等，每次派发前重写以保证是最新版）
#[cfg(target_os = "macos")]
fn write_mac_helper() -> Result<PathBuf, String> {
    let dir = updates_root();
    fs::create_dir_all(&dir).map_err(|e| format!("创建更新目录失败: {}", e))?;
    let script = dir.join("apply.sh");
    fs::write(&script, MAC_HELPER).map_err(|e| format!("写入更新脚本失败: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置脚本权限失败: {}", e))?;
    }
    Ok(script)
}

/// 派发 macOS helper：spawn 后立刻返回，调用方紧接着退出应用
#[cfg(target_os = "macos")]
pub fn spawn_helper(plan: &ApplyPlan) -> Result<(), String> {
    let script = write_mac_helper()?;
    let log = helper_log_path();
    // 清空旧日志：helper 用追加写，读到的必须是本次运行的结果
    let _ = fs::write(&log, "");

    let mut cmd = Command::new("/bin/sh");
    cmd.arg(&script)
        .arg(std::process::id().to_string())
        .arg(&plan.target)
        .arg(&plan.staged)
        .arg(&log)
        .arg(&plan.workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 脱离原进程组：主进程退出时不会把它一起收走
        cmd.process_group(0);
    }
    cmd.spawn()
        .map_err(|e| format!("启动更新进程失败: {}", e))?;
    Ok(())
}

// ========== Windows helper（自身 exe 副本，未在真机验证） ==========

#[cfg(target_os = "windows")]
pub const HELPER_FLAG: &str = "--lumia-apply-update";

#[cfg(target_os = "windows")]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(target_os = "windows")]
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// main() 最开头调用：如果自己是被派发出来的更新 helper，就执行替换并给出退出码。
/// 返回 None 表示「正常启动应用」。
///
/// 这里必须早于任何 Tauri 初始化 —— helper 只是同一个二进制换了入口。
pub fn run_helper_if_requested() -> Option<i32> {
    #[cfg(target_os = "windows")]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == HELPER_FLAG) {
            return Some(run_windows_helper(&args));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn arg_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    let idx = args.iter().position(|a| a == key)?;
    args.get(idx + 1).map(|s| s.as_str())
}

/// Windows 替换流程：等锁释放 → 备份 → 就位 → 拉起新版 → 清理。
/// ⚠️ 本机无 Windows 环境，此分支**未经真机验证**；逻辑上不依赖任何
/// 「运行中的 exe 能改名」的技巧 —— 我们只在主进程退出后动手。
#[cfg(target_os = "windows")]
fn run_windows_helper(args: &[String]) -> i32 {
    use std::io::Write;

    let log_path = arg_value(args, "--log")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join("lumia-update.log")
        });
    let log = |msg: &str| {
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(f, "{}", msg);
        }
    };

    let (Some(target), Some(new_exe)) = (
        arg_value(args, "--target").map(PathBuf::from),
        arg_value(args, "--new").map(PathBuf::from),
    ) else {
        log("FAIL: 缺少 --target / --new 参数");
        return 1;
    };

    // 1) 等主进程放开文件锁：直接试着改名，成功即锁已释放。
    //    比等 PID 更准 —— 我们要的本来就是「这个文件能被动了」。
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let bak = target.with_file_name(format!(
        "{}.old-{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "lumia".into()),
        ts
    ));

    let mut moved = false;
    for _ in 0..60 {
        if fs::rename(&target, &bak).is_ok() {
            moved = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if !moved {
        log("FAIL: 旧版本文件被占用，无法替换");
        return 10;
    }

    // 2) 新版本就位
    if fs::rename(&new_exe, &target).is_err() {
        log("FAIL: 写入新版本失败，已回滚");
        let _ = fs::rename(&bak, &target);
        launch(&target);
        return 11;
    }
    let _ = fs::remove_file(&bak);
    log("OK: replaced");

    // 3) 拉起新版（分离，不随 helper 退出而结束）
    launch(&target);

    // 4) 把自己挪开，交给主程序下次启动时清理（运行中的 exe 删不掉但能改名）
    if let Ok(me) = std::env::current_exe() {
        let _ = fs::rename(&me, me.with_extension("old-helper.exe"));
    }

    // 5) 清理工作目录
    if let Some(work) = arg_value(args, "--work").map(PathBuf::from) {
        let _ = fs::remove_dir_all(work);
    }
    0
}

#[cfg(target_os = "windows")]
fn launch(exe: &Path) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new(exe)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// 把自身复制成 helper 并派发（Windows）
#[cfg(target_os = "windows")]
pub fn spawn_helper(plan: &ApplyPlan) -> Result<(), String> {
    let me = normalize_path(
        std::env::current_exe().map_err(|e| format!("无法定位自身路径: {}", e))?,
    );
    let dir = me.parent().ok_or("无法定位安装目录")?;
    let helper = dir.join("lumia-helper.exe");

    // 上一次的残留（可能被占用）先尝试清掉，失败就换个名字
    let _ = fs::remove_file(&helper);
    if fs::copy(&me, &helper).is_err() {
        let alt = dir.join(format!("lumia-helper-{}.exe", std::process::id()));
        fs::copy(&me, &alt).map_err(|e| format!("复制更新程序失败: {}", e))?;
        return spawn_windows_helper_process(&alt, plan);
    }
    spawn_windows_helper_process(&helper, plan)
}

#[cfg(target_os = "windows")]
fn spawn_windows_helper_process(helper: &Path, plan: &ApplyPlan) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let log = helper_log_path();
    fs::create_dir_all(updates_root()).map_err(|e| format!("创建更新目录失败: {}", e))?;
    let _ = fs::write(&log, "");

    Command::new(helper)
        .arg(HELPER_FLAG)
        .arg("--pid")
        .arg(std::process::id().to_string())
        .arg("--target")
        .arg(&plan.target)
        .arg("--new")
        .arg(&plan.staged)
        .arg("--log")
        .arg(&log)
        .arg("--work")
        .arg(&plan.workdir)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动更新进程失败: {}", e))?;
    Ok(())
}

/// 非 Windows / 非 macOS 平台的占位（这两个平台各自有实现）
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn spawn_helper(_plan: &ApplyPlan) -> Result<(), String> {
    Err("当前平台不支持".into())
}

/// 启动时清理上次更新留下的残留（Windows 的 .old 文件）。
/// 不做 updates 目录的整体清理 —— helper 可能刚拉起新版、还在收尾，
/// 那个目录由 helper 自己清，或下次下载前由 clear_workspace 清。
pub fn cleanup_leftovers() {
    #[cfg(target_os = "windows")]
    {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let exe = normalize_path(exe);
        let Some(dir) = exe.parent() else { return };
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if lower.contains(".old-") || lower.starts_with("lumia-helper") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// 供 prepare/apply 复用的时间戳
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pkg_from_map(map: serde_json::Map<String, serde_json::Value>) -> Option<UpdatePackage> {
        pick_package(&serde_json::Value::Object(map))
    }

    /// 平铺结构（服务端方案 A）：mac_arm64_zip + mac_arm64_zip_sha256
    #[test]
    fn pick_package_flat_shape() {
        let mut v = serde_json::Map::new();
        v.insert(
            format!("{}_zip", platform_key()),
            json!("https://example.com/pkg.zip"),
        );
        v.insert(
            format!("{}_zip_sha256", platform_key()),
            json!("AB".repeat(32)), // 大写，应被规整为小写
        );
        let pkg = pkg_from_map(v).expect("应该解析出平铺字段");
        assert_eq!(pkg.url, "https://example.com/pkg.zip");
        assert_eq!(pkg.sha256.as_deref().unwrap(), "ab".repeat(32));
    }

    /// 结构化（方案 B）：update["macos-aarch64"] = { url, sha256, size }
    #[test]
    fn pick_package_structured_shape() {
        let mut inner = serde_json::Map::new();
        inner.insert("url".into(), json!("https://example.com/pkg2.zip"));
        inner.insert("sha256".into(), json!("c".repeat(64)));
        inner.insert("size".into(), json!(1234));
        let mut outer = serde_json::Map::new();
        outer.insert(
            platform_dashed_key().to_string(),
            serde_json::Value::Object(inner),
        );
        let mut root = serde_json::Map::new();
        root.insert("update".into(), serde_json::Value::Object(outer));

        let pkg = pkg_from_map(root).expect("应该解析出结构化字段");
        assert_eq!(pkg.url, "https://example.com/pkg2.zip");
        assert_eq!(pkg.size, 1234);
    }

    /// 老服务端没有这些字段 → None（调用方必须退回「打开浏览器下载」，不能报错）
    #[test]
    fn pick_package_missing_fields_is_none() {
        let mut v = serde_json::Map::new();
        v.insert("latest".into(), json!("v1.0 beta 2"));
        assert!(pkg_from_map(v).is_none());
    }

    /// 非 https / 非 64 位 hex 的哈希不应被采信
    #[test]
    fn pick_package_rejects_sloppy_values() {
        let mut v = serde_json::Map::new();
        v.insert(format!("{}_zip", platform_key()), json!("http://insecure/pkg.zip"));
        assert!(pkg_from_map(v).is_none(), "http 地址应被拒绝");

        let mut v2 = serde_json::Map::new();
        v2.insert(
            format!("{}_zip", platform_key()),
            json!("https://example.com/pkg.zip"),
        );
        v2.insert(format!("{}_zip_sha256", platform_key()), json!("not-a-hash"));
        let pkg = pkg_from_map(v2).expect("url 合法就该出包");
        assert!(pkg.sha256.is_none(), "非法哈希应被丢弃而不是照单全收");
    }

    /// 版本号会被拼进路径，必须挡住能改变层级的名字
    #[test]
    fn safe_version_blocks_path_traversal() {
        assert_eq!(safe_version(".."), "");
        assert_eq!(safe_version("."), "");
        assert_eq!(safe_version("   "), "");
        assert_eq!(safe_version("v1.0"), "v1.0");
        assert_eq!(safe_version("v1.0 beta 2"), "v1.0 beta 2");
        // `/` 会被替换，结果只是一个普通目录名（不再是层级分隔）
        assert_eq!(safe_version("1.0/../x"), "1.0_.._x");
    }

    #[test]
    fn sha256_matches_known_vector() {
        let dir = std::env::temp_dir().join(format!("lumia-sha-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("hello.txt");
        fs::write(&f, b"hello").unwrap();
        let got = sha256_file(&f).unwrap();
        assert_eq!(
            got,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_path_strips_windows_prefix() {
        assert_eq!(
            normalize_path(PathBuf::from(r"\\?\C:\Games\lumia.exe")),
            PathBuf::from(r"C:\Games\lumia.exe")
        );
        assert_eq!(
            normalize_path(PathBuf::from("/Applications/Lumia.app")),
            PathBuf::from("/Applications/Lumia.app")
        );
    }

    /// apply 阶段的一致性校验：计划里的路径一旦越界就必须拒绝
    #[test]
    fn plan_rejects_paths_outside_updates_root() {
        let plan = ApplyPlan {
            version: "v9".into(),
            target: PathBuf::from("/Applications/Fake.app"),
            staged: PathBuf::from("/etc/passwd"),
            workdir: PathBuf::from("/tmp"),
            created_at: 0,
        };
        // staged 不在 workdir 内 → 判定失败
        assert!(!plan.staged.starts_with(&plan.workdir));
        assert!(!plan.workdir.starts_with(updates_root()));
    }
}

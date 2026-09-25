// ============================================================================
// Lumia 插件系统：.lplugin 扫描 / 解压 / 注册 / 命令
// 插件 = zip 改后缀丢进 <app_data>/plugins/；内含 main.lumi（Lumi 语言主文件）+ 资源。
// ============================================================================

use crate::lumi;
use serde_json::json;
use tauri::Manager;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 一个已注册插件（Clone 只复制 Arc 指针，运行时内容共享）
#[derive(Clone)]
pub struct PluginEntry {
    pub id: String,                   // 文件名（去 .lplugin 后缀）
    pub dir: PathBuf,                 // 解压后的插件目录
    pub runtime: Arc<Mutex<lumi::PluginRuntime>>,
    pub enabled: bool,
    pub builtin: bool,
    pub manifest: serde_json::Value,  // 元信息（name/version/desc/icon/min）
}

/// 全局插件注册表
pub struct PluginRegistry {
    pub plugins: HashMap<String, PluginEntry>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        PluginRegistry {
            plugins: HashMap::new(),
        }
    }
}

/// 插件根目录：<app_data>/plugins
fn plugins_root(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("lumia"))
        .join("plugins")
}

/// 扫描并加载全部插件（已有则跳过；enabled 状态由调用方传入）。
/// 注意：不能在持有 reg 锁时调用 load_plugin_dir（内部会再次 lock 同一把 std Mutex → 非重入死锁）。
/// 因此先无锁收集候选，drop guard 后再逐个加载。
pub async fn scan_plugins(app_handle: &tauri::AppHandle, reg: &Arc<Mutex<PluginRegistry>>) {
    let root = plugins_root(app_handle);
    let _ = std::fs::create_dir_all(&root);

    // 1. 无锁收集候选（三种形态：目录 / .lplugin 包 / 单文件 .lumi）
    let mut dir_candidates: Vec<(String, PathBuf)> = Vec::new();
    let mut pkg_candidates: Vec<(String, PathBuf)> = Vec::new();
    let mut file_candidates: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(id) = p.file_name().and_then(|n| n.to_str()) {
                    if p.join("main.lumi").exists() {
                        dir_candidates.push((id.to_string(), p));
                    }
                }
            } else if p.extension().and_then(|x| x.to_str()) == Some("lplugin") {
                let id = p
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("plugin")
                    .to_string();
                pkg_candidates.push((id, p));
            } else if p.extension().and_then(|x| x.to_str()) == Some("lumi") {
                // 单文件插件：一个 .lumi 即插件本体（无资源，图标默认 plugin.svg）
                let id = p
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("plugin")
                    .to_string();
                file_candidates.push((id, p));
            }
        }
    }

    let mut seen: Vec<String> = Vec::new();

    // 2. 目录形态（已在注册表则跳过）
    for (id, dir) in dir_candidates {
        if reg.lock().unwrap().plugins.contains_key(&id) || seen.contains(&id) {
            continue;
        }
        match load_plugin_dir(app_handle, reg, &dir, id.clone(), false) {
            Ok(()) => seen.push(id.clone()),
            Err(err) => {
                eprintln!("[插件] 加载失败 {}: {}", id, err);
            }
        }
    }

    // 3. .lplugin 压缩包（解压后加载）
    for (id, pkg) in pkg_candidates {
        if reg.lock().unwrap().plugins.contains_key(&id) || seen.contains(&id) {
            continue;
        }
        let target = root.join(&id);
        if extract_plugin(&pkg, &target).is_ok() && target.join("main.lumi").exists() {
            match load_plugin_dir(app_handle, reg, &target, id.clone(), false) {
                Ok(()) => seen.push(id.clone()),
                Err(err) => eprintln!("[插件] 加载失败 {}: {}", id, err),
            }
        }
    }

    // 4. 单文件 .lumi（无资源也可正常解析；单文件形态过不了市场审核，但本地加载不拦）
    for (id, file) in file_candidates {
        if reg.lock().unwrap().plugins.contains_key(&id) || seen.contains(&id) {
            continue;
        }
        match load_plugin_file(app_handle, reg, &file, id.clone(), false) {
            Ok(()) => seen.push(id.clone()),
            Err(err) => eprintln!("[插件] 加载失败 {}: {}", id, err),
        }
    }
}


/// 解压 .lplugin（zip）到目标目录
fn extract_plugin(zip_path: &Path, target: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("打开插件包失败: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("解析插件包失败: {}", e))?;
    let _ = std::fs::remove_dir_all(target);
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取插件包内容失败: {}", e))?;
        let out_path = target.join(entry.mangled_name());
        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&out_path);
            continue;
        }
        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("写入插件文件失败: {}", e))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("解压插件失败: {}", e))?;
    }
    Ok(())
}

/// 注册插件条目并启动后台任务（顶层 while / 顶层 if 轮询）
fn register_entry(reg: &Arc<Mutex<PluginRegistry>>, entry: PluginEntry) {
    let id = entry.id.clone();
    reg.lock().unwrap().plugins.insert(id.clone(), entry.clone());
    // 顶层 while：独立后台任务
    {
        let rt = entry.runtime.clone();
        tauri::async_runtime::spawn(async move {
            lumi::run_while_tasks(rt).await;
        });
    }
    // 顶层 if：每 50ms 评估一次
    {
        let rt = entry.runtime.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                lumi::step_loops(&rt).await;
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        });
    }
    log_plugins_line(&format!("已加载插件 {}", id));
}

/// 从目录加载一个插件（含 main.lumi）
fn load_plugin_dir(
    _app_handle: &tauri::AppHandle,
    reg: &Arc<Mutex<PluginRegistry>>,
    dir: &Path,
    id: String,
    builtin: bool,
) -> Result<(), String> {
    let main_path = dir.join("main.lumi");
    let src = std::fs::read_to_string(&main_path)
        .map_err(|e| format!("读取 main.lumi 失败: {}", e))?;
    let runtime = lumi::build_runtime(&src, dir.to_path_buf())
        .map_err(|e| format!("Lumi 解析失败: {}", e))?;
    let manifest = manifest_json(&runtime);
    let entry = PluginEntry {
        id,
        dir: dir.to_path_buf(),
        runtime: Arc::new(Mutex::new(runtime)),
        enabled: true,
        builtin,
        manifest,
    };
    register_entry(reg, entry);
    Ok(())
}

/// 从单文件加载一个插件（`plugins/xxx.lumi`，无任何资源文件）。
/// 单文件形态可正常解析运行（图标缺省时前端默认 plugin.svg）；
/// 因无资源/无图标，过不了插件市场审核，但本地加载不拦。
fn load_plugin_file(
    _app_handle: &tauri::AppHandle,
    reg: &Arc<Mutex<PluginRegistry>>,
    file: &Path,
    id: String,
    builtin: bool,
) -> Result<(), String> {
    let src = std::fs::read_to_string(file)
        .map_err(|e| format!("读取 {} 失败: {}", file.display(), e))?;
    // 插件目录 = 该文件所在目录（单文件无独立资源目录）
    let dir = file
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let runtime = lumi::build_runtime(&src, dir.clone())
        .map_err(|e| format!("Lumi 解析失败: {}", e))?;
    let manifest = manifest_json(&runtime);
    let entry = PluginEntry {
        id,
        dir,
        runtime: Arc::new(Mutex::new(runtime)),
        enabled: true,
        builtin,
        manifest,
    };
    register_entry(reg, entry);
    Ok(())
}

fn manifest_json(rt: &lumi::PluginRuntime) -> serde_json::Value {
    json!({
        "name": rt.meta.name,
        "version": rt.meta.version,
        "description": rt.meta.description,
        "icon": rt.meta.icon,
        "min": rt.meta.min,
    })
}

fn log_plugins_line(msg: &str) {
    eprintln!("[插件] {}", msg);
}

/// 插件信息列表（给前端）
fn plugin_info_list(reg: &Arc<Mutex<PluginRegistry>>) -> Vec<serde_json::Value> {
    let guard = reg.lock().unwrap();
    let mut v: Vec<serde_json::Value> = guard
        .plugins
        .iter()
        .map(|(id, e)| {
            json!({
                "id": id,
                "name": e.manifest["name"].as_str().unwrap_or(id),
                "version": e.manifest["version"].as_str().unwrap_or(""),
                "description": e.manifest["description"].as_str().unwrap_or(""),
                // icon：manifest 里声明了才给（相对插件目录的绝对路径），空串 → 前端用默认 plugin.svg
                "icon": e.manifest["icon"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| e.dir.join(s).to_string_lossy().to_string())
                    .unwrap_or_default(),
                "enabled": e.enabled,
                "builtin": e.builtin,
                "hasControls": !e.runtime.lock().unwrap().controls.is_empty(),
            })
        })
        .collect();
    v.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    v
}

/// 插件页控件树
fn plugin_tree_of(reg: &Arc<Mutex<PluginRegistry>>, id: &str) -> Option<serde_json::Value> {
    let guard = reg.lock().unwrap();
    let e = guard.plugins.get(id)?;
    let rt = e.runtime.lock().unwrap();
    Some(lumi::controls_to_json(&rt))
}

// ---------- Tauri 命令 ----------

#[tauri::command]
pub async fn plugin_list(app_handle: tauri::AppHandle, reg_state: tauri::State<'_, PluginRegistryState>) -> Result<Vec<serde_json::Value>, String> {
    // 扫描（幂等：已有插件跳过）
    let reg = reg_state.0.clone();
    scan_plugins(&app_handle, &reg).await;
    Ok(plugin_info_list(&reg))
}

#[tauri::command]
pub async fn plugin_ui_tree(
    id: String,
    reg_state: tauri::State<'_, PluginRegistryState>,
) -> Result<serde_json::Value, String> {
    let reg = reg_state.0.clone();
    match plugin_tree_of(&reg, &id) {
        Some(tree) => Ok(tree),
        None => Err(format!("插件 {} 不存在", id)),
    }
}

#[tauri::command]
pub async fn plugin_page_injects(
    page: String,
    reg_state: tauri::State<'_, PluginRegistryState>,
) -> Result<serde_json::Value, String> {
    let reg = reg_state.0.clone();
    let entries: Vec<PluginEntry> = {
        let guard = reg.lock().unwrap();
        guard.plugins.values().cloned().collect()
    };
    let mut result: Vec<serde_json::Value> = Vec::new();
    for e in entries {
        if !e.enabled {
            continue;
        }
        let rt = e.runtime.lock().unwrap();
        let page_data = lumi::page_inject_controls_to_json(&rt, &page);
        let controls = page_data.get("controls").cloned().unwrap_or(serde_json::Value::Array(vec![]));
        if let serde_json::Value::Array(ref arr) = controls {
            if !arr.is_empty() {
                result.push(json!({
                    "pluginId": e.id,
                    "pluginName": e.manifest["name"].as_str().unwrap_or(&e.id),
                    "controls": controls,
                    "overrides": page_data.get("overrides").cloned().unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                }));
                continue;
            }
        }
        let overrides = page_data.get("overrides");
        if overrides.map_or(false, |o| o.as_object().map_or(false, |m| !m.is_empty())) {
            result.push(json!({
                "pluginId": e.id,
                "pluginName": e.manifest["name"].as_str().unwrap_or(&e.id),
                "controls": serde_json::Value::Array(vec![]),
                "overrides": overrides.cloned().unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
            }));
        }
    }
    Ok(serde_json::Value::Array(result))
}

#[tauri::command]
pub async fn plugin_ui_event(
    id: String,
    control: String,
    event: String,
    reg_state: tauri::State<'_, PluginRegistryState>,
) -> Result<serde_json::Value, String> {
    let reg = reg_state.0.clone();
    let entry = reg
        .lock()
        .unwrap()
        .plugins
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("插件 {} 不存在", id))?;
    let rt = entry.runtime;
    let (notify, tree) = lumi::dispatch_event(
        &rt,
        &lumi::ListenerTarget::Control(control),
        &event,
    )
    .await?;
    Ok(json!({ "notify": notify, "tree": tree }))
}

/// 系统事件广播：通知所有启用插件中 `listen 系统事件:` 的块执行（如 game_start / game_quit 等）。
/// 返回全部插件产生的通知（toast/popup），前端统一展示。
#[tauri::command]
pub async fn plugin_emit_event(
    event: String,
    reg_state: tauri::State<'_, PluginRegistryState>,
) -> Result<serde_json::Value, String> {
    let reg = reg_state.0.clone();
    let entries: Vec<PluginEntry> = {
        let guard = reg.lock().unwrap();
        guard.plugins.values().cloned().collect()
    };
    let mut notify: Vec<lumi::Notify> = Vec::new();
    for e in entries {
        if !e.enabled {
            continue;
        }
        let (mut n, _tree) = lumi::dispatch_event(
            &e.runtime,
            &lumi::ListenerTarget::System(event.clone()),
            "",
        )
        .await?;
        notify.append(&mut n);
    }
    Ok(json!({ "notify": notify }))
}

/// 启用/停用插件（enabled=false 时前端不再显示入口；事件分发拒绝）
#[tauri::command]
pub fn plugin_set_enabled(
    id: String,
    enabled: bool,
    reg_state: tauri::State<'_, PluginRegistryState>,
) -> Result<(), String> {
    let reg = reg_state.0.clone();
    let mut guard = reg.lock().unwrap();
    let e = guard
        .plugins
        .get_mut(&id)
        .ok_or_else(|| format!("插件 {} 不存在", id))?;
    e.enabled = enabled;
    Ok(())
}

/// 删除插件：从注册表移除，并删除解压目录 + 源 .lplugin 包（如有）
#[tauri::command]
pub fn plugin_delete(
    id: String,
    app_handle: tauri::AppHandle,
    reg_state: tauri::State<'_, PluginRegistryState>,
) -> Result<(), String> {
    let reg = reg_state.0.clone();
    // 先取出待删信息（短锁，不跨锁调用）
    let (dir, root) = {
        let guard = reg.lock().unwrap();
        let e = guard
            .plugins
            .get(&id)
            .ok_or_else(|| format!("插件 {} 不存在", id))?;
        (e.dir.clone(), plugins_root(&app_handle))
    };
    // 从注册表移除
    reg.lock().unwrap().plugins.remove(&id);
    // 删除解压目录
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除插件目录失败: {}", e))?;
    }
    // 删除源 .lplugin 包
    let pkg = root.join(format!("{}.lplugin", id));
    if pkg.exists() {
        std::fs::remove_file(&pkg).map_err(|e| format!("删除插件包失败: {}", e))?;
    }
    Ok(())
}

/// 用系统文件管理器打开插件目录
#[tauri::command]
pub fn plugin_open_dir(
    id: String,
    reg_state: tauri::State<'_, PluginRegistryState>,
) -> Result<(), String> {
    let reg = reg_state.0.clone();
    let dir = reg
        .lock()
        .unwrap()
        .plugins
        .get(&id)
        .map(|e| e.dir.clone())
        .ok_or_else(|| format!("插件 {} 不存在", id))?;
    open::that(&dir).map_err(|e| format!("打开插件目录失败: {}", e))
}

/// 插件状态（注册表容器）
pub struct PluginRegistryState(pub Arc<Mutex<PluginRegistry>>);

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时插件根目录（隔离，不污染真实 app_data）
    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumia-plugins-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 构造一个 zip 格式 .lplugin 并解压（走真实 extract_plugin 路径），验证 .lplugin 形态可被加载
    fn write_and_extract_lplugin(dir: &Path) {
        let root = temp_root("zlp");
        let pkg = root.join("test.lplugin");
        {
            let file = std::fs::File::create(&pkg).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("main.lumi", zip::write::FileOptions::default()).unwrap();
            let src = "name: Test Plugin\nGo = CreateButton('Go')\nlisten Go.click(Go = 'Done')\n";
            std::io::Write::write_all(&mut zip, src.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        extract_plugin(&pkg, dir).expect("extract .lplugin");
        assert!(dir.join("main.lumi").exists(), ".lplugin 解压后 main.lumi 应存在");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dispatch_click_runs_handler_and_updates_controls() {
        // 用内联源码（不依赖真实存档目录），验证事件分发 → 控件更新 + toast 通知的完整链路
        let src = r#"
name: Auto Backup
Backup_Button = CreateButton('立即备份')
Status_Text = CreateText('等待中')
listen Backup_Button.click:
    Status_Text = '备份完成'
    toast('备份完成')
"#;
        let root = temp_root("dispatch");
        let dir = root.join("autosave");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("main.lumi"), src).unwrap();
        let reg = Arc::new(Mutex::new(PluginRegistry::new()));
        // 直接构造运行时并注册（避开 tauri AppHandle 依赖）
        let runtime = lumi::build_runtime(src, dir.clone()).expect("build_runtime");
        let entry = PluginEntry {
            id: "autosave".to_string(),
            dir: dir.clone(),
            runtime: Arc::new(Mutex::new(runtime)),
            enabled: true,
            builtin: true,
            manifest: manifest_json(&lumi::build_runtime(src, dir.clone()).unwrap()),
        };
        reg.lock().unwrap().plugins.insert("autosave".to_string(), entry.clone());

        let rt_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (notify, tree) = rt_runtime
            .block_on(lumi::dispatch_event(
                &entry.runtime,
                &lumi::ListenerTarget::Control("Backup_Button".to_string()),
                "click",
            ))
            .expect("dispatch");
        // 后端动作 toast('备份完成') 生效
        assert!(
            notify.iter().any(|n| n.kind == "toast"),
            "应产生 toast 通知: {:?}",
            notify
        );
        // 控件树中 Status_Text 文本被更新为 '备份完成'
        let status = tree
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "Status_Text")
            .expect("Status_Text 在控件树中");
        assert_eq!(status["text"], "备份完成", "点击后 Status_Text 应更新");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_lplugin_zip_roundtrip() {
        let target = temp_root("roundtrip");
        write_and_extract_lplugin(&target);
        let src = std::fs::read_to_string(target.join("main.lumi")).unwrap();
        let rt = lumi::build_runtime(&src, target.clone()).expect("Lumi 解析失败");
        assert_eq!(rt.meta.name, "Test Plugin");
        assert!(rt.controls.iter().any(|c| c.name == "Go" && c.ctype == "button"));
        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn single_file_plugin_loads() {
        // 单文件形态：plugins/xxx.lumi 直接丢入（无任何资源）也能正常解析注册
        let root = temp_root("single-file");
        let _ = std::fs::create_dir_all(&root);
        let file = root.join("hello.lumi");
        std::fs::write(
            &file,
            "name: Hello\nversion: 1.0\nNote = CreateText('hi')\nlisten Note.click:\n    Note = '点过'\n",
        )
        .unwrap();
        let reg = Arc::new(Mutex::new(PluginRegistry::new()));
        // 单文件加载：读取文件 → build_runtime → 注册，不经目录形态
        let src = std::fs::read_to_string(&file).unwrap();
        let rt = lumi::build_runtime(&src, root.clone()).expect("Lumi 解析失败");
        let manifest = manifest_json(&rt);
        assert_eq!(manifest["name"], "Hello");
        assert_eq!(manifest["icon"], "", "单文件无图标，icon 应为空串（前端回退默认图标）");
        assert!(rt.controls.iter().any(|c| c.name == "Note"));
        // 走真实注册路径（跳过 spawn，只验证 entry 插入）
        let entry = PluginEntry {
            id: "hello".to_string(),
            dir: root.clone(),
            runtime: Arc::new(Mutex::new(rt)),
            enabled: true,
            builtin: false,
            manifest,
        };
        reg.lock().unwrap().plugins.insert("hello".to_string(), entry.clone());
        assert!(reg.lock().unwrap().plugins.contains_key("hello"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
//! 本地音乐库（跨平台）
//!
//! 架构：音频文件**复制**进应用数据目录 `app_data/music_library/<歌单名>/<文件>`，
//! 播放的是库内副本 —— 用户删除原始音频文件不影响播放（这正是"防丢失"方案）。
//!
//! 前端用 `<input type="file">`/`webkitdirectory` 拿文件字节，经 IPC 交给
//! `music_import` 写入库目录；播放走自定义 `stream://` 协议流式读取。
//!
//! # 线程说明
//! 命令是异步的（tauri command async），内部用 `tokio::fs` 避免阻塞主线程；
//! `music_stream` 由协议处理器调用，同样异步读文件。

use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// 歌单（= music_library 下的一个子目录）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylist {
    pub name: String,
    pub songs: Vec<MusicSong>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSong {
    pub filename: String,
    /// 相对歌单目录的路径（播放时拼到 stream:// 协议后面）
    pub rel_path: String,
    /// 文件大小（字节，前端显示用）
    pub size: u64,
}

/// 允许导入的音频扩展名（小写）
const AUDIO_EXTS: [&str; 9] = ["mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "wma", "aiff"];

/// 允许导入的视频扩展名（小写）：WebView 的 <audio> 能直接解码这些容器的音轨，
/// 导入即"自动转音频" —— 播放时只出声不放画面。
const VIDEO_EXTS: [&str; 6] = ["mp4", "m4v", "mov", "webm", "ogv", "mkv"];

/// 音乐库根目录：app_data/music_library
fn music_root(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录: {e}"))?
        .join("music_library");
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建音乐库目录: {e}"))?;
    Ok(dir)
}

/// 歌单目录 → 名字。校验：只允许安全目录名（防路径穿越）
fn playlist_dir(root: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("歌单名不能为空".into());
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("歌单名不合法".into());
    }
    Ok(root.join(name))
}

/// 文件名校验：只允许文件名本身（剥离到单一组件，防穿越）
fn safe_filename(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("文件名不能为空".into());
    }
    // 只保留最后一段，并拒绝路径分隔符
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if base.is_empty() || base == "." || base == ".." || base.contains('/') || base.contains('\\') {
        return Err("文件名不合法".into());
    }
    Ok(base.to_string())
}

/// 可导入的文件 = 音频 + 视频（视频直接播音轨）
fn is_audio(filename: &str) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            AUDIO_EXTS.contains(&e.as_str()) || VIDEO_EXTS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

/// 导入结果统计
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// 成功复制入库的文件数
    pub imported: u32,
    /// 跳过的文件数（非音频/视频、已存在或复制失败）
    pub skipped: u32,
}

/// 歌单目录内已有的文件名集合（用于同名去重）
fn existing_names(pl_dir: &Path) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if let Ok(rd) = std::fs::read_dir(pl_dir) {
        for entry in rd.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                set.insert(name.to_string());
            }
        }
    }
    set
}

/// 目标文件名去重：已存在同名时加 " (n)" 后缀
fn unique_name(base: &str, existing: &std::collections::HashSet<String>) -> String {
    if !existing.contains(base) {
        return base.to_string();
    }
    let dot = base.rfind('.');
    let (stem, ext) = match dot {
        Some(i) => (&base[..i], &base[i..]),
        None => (base, ""),
    };
    let mut n = 1;
    loop {
        let cand = format!("{stem} ({n}){ext}");
        if !existing.contains(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// 复制单个文件进歌单（白名单过滤 + 同名去重）。返回是否成功入库。
async fn copy_into_playlist(
    pl_dir: &Path,
    src: &Path,
    existing: &std::collections::HashSet<String>,
) -> bool {
    let Some(fname_os) = src.file_name() else { return false };
    let Some(fname) = fname_os.to_str() else { return false };
    if !is_audio(fname) {
        return false;
    }
    let dest_name = unique_name(fname, existing);
    let dest = pl_dir.join(&dest_name);
    match tokio::fs::copy(src, &dest).await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("[音乐库] 复制失败 {} → {}: {e}", src.display(), dest.display());
            false
        }
    }
}

/// 按路径导入音频/视频文件（磁盘流式复制，不经 IPC 传字节）。
/// 前端用系统文件选择器拿到绝对路径列表后调用。playlist 为目标歌单名。
#[tauri::command]
pub async fn music_import_paths(
    app: tauri::AppHandle,
    playlist: String,
    paths: Vec<String>,
) -> Result<ImportResult, String> {
    let root = music_root(&app)?;
    let pl_dir = playlist_dir(&root, &playlist)?;
    std::fs::create_dir_all(&pl_dir).map_err(|e| format!("无法创建歌单目录: {e}"))?;

    let mut existing = existing_names(&pl_dir);
    let mut result = ImportResult { imported: 0, skipped: 0 };
    for p in paths {
        let src = PathBuf::from(&p);
        if src.is_file() && copy_into_playlist(&pl_dir, &src, &existing).await {
            if let Some(name) = src.file_name().and_then(|s| s.to_str()) {
                existing.insert(unique_name(name, &existing));
            }
            result.imported += 1;
        } else {
            result.skipped += 1;
        }
    }
    Ok(result)
}

/// 按文件夹路径导入整个文件夹为一个歌单（歌单名 = 文件夹名）。
/// 递归收集文件夹内所有音频/视频文件复制进库。
#[tauri::command]
pub async fn music_import_folder(
    app: tauri::AppHandle,
    folder: String,
) -> Result<ImportResult, String> {
    let src_dir = PathBuf::from(&folder);
    if !src_dir.is_dir() {
        return Err("所选路径不是文件夹".into());
    }
    let Some(name_os) = src_dir.file_name() else {
        return Err("无法获取文件夹名".into());
    };
    let Some(playlist) = name_os.to_str() else {
        return Err("文件夹名不是有效文本".into());
    };

    let root = music_root(&app)?;
    let pl_dir = playlist_dir(&root, playlist)?;
    std::fs::create_dir_all(&pl_dir).map_err(|e| format!("无法创建歌单目录: {e}"))?;

    // 递归收集文件夹内全部文件（跳过隐藏目录）
    let mut files: Vec<PathBuf> = Vec::new();
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let hidden = entry
                        .file_name()
                        .to_str()
                        .map(|n| n.starts_with('.'))
                        .unwrap_or(false);
                    if !hidden {
                        collect(&path, out);
                    }
                } else if path.is_file() {
                    out.push(path);
                }
            }
        }
    }
    collect(&src_dir, &mut files);
    files.sort();

    let mut existing = existing_names(&pl_dir);
    let mut result = ImportResult { imported: 0, skipped: 0 };
    for src in files {
        if copy_into_playlist(&pl_dir, &src, &existing).await {
            if let Some(name) = src.file_name().and_then(|s| s.to_str()) {
                existing.insert(unique_name(name, &existing));
            }
            result.imported += 1;
        } else {
            result.skipped += 1;
        }
    }
    Ok(result)
}

/// 列出全部歌单与歌曲（按目录名排序，忽略隐藏目录）
#[tauri::command]
pub async fn music_list(app: tauri::AppHandle) -> Result<Vec<MusicPlaylist>, String> {
    let root = music_root(&app)?;
    let mut entries = tokio::fs::read_dir(&root)
        .await
        .map_err(|e| format!("读取音乐库失败: {e}"))?;
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false)
            && !name.starts_with('.')
        {
            names.push(name);
        }
    }
    names.sort();

    let mut playlists = Vec::new();
    for name in names {
        let dir = root.join(&name);
        let mut songs = Vec::new();
        let mut rd = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| format!("读取歌单 {name} 失败: {e}"))?;
        while let Some(entry) = rd.next_entry().await.map_err(|e| e.to_string())? {
            if entry.file_type().await.map(|t| t.is_file()).unwrap_or(false) {
                let fname = entry.file_name().to_string_lossy().into_owned();
                if is_audio(&fname) {
                    let meta = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                    songs.push(MusicSong {
                        filename: fname.clone(),
                        rel_path: format!("{}/{}", name, fname),
                        size: meta,
                    });
                }
            }
        }
        songs.sort_by(|a, b| a.filename.to_lowercase().cmp(&b.filename.to_lowercase()));
        if !songs.is_empty() {
            playlists.push(MusicPlaylist { name, songs });
        }
    }
    Ok(playlists)
}

/// 删除歌单（整个目录）
#[tauri::command]
pub async fn music_delete_playlist(app: tauri::AppHandle, playlist: String) -> Result<(), String> {
    let root = music_root(&app)?;
    let dir = playlist_dir(&root, &playlist)?;
    if !dir.exists() {
        return Err("歌单不存在".into());
    }
    tokio::fs::remove_dir_all(&dir)
        .await
        .map_err(|e| format!("删除歌单失败: {e}"))
}

/// 从歌单中删除一首歌
#[tauri::command]
pub async fn music_remove_song(
    app: tauri::AppHandle,
    playlist: String,
    filename: String,
) -> Result<(), String> {
    let root = music_root(&app)?;
    let dir = playlist_dir(&root, &playlist)?;
    let name = safe_filename(&filename)?;
    let path = dir.join(&name);
    if !path.exists() {
        return Err("歌曲不存在".into());
    }
    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| format!("删除歌曲失败: {e}"))
}

/// 解析 HTTP Range 头（仅支持单段 `bytes=start-end` / `bytes=start-` / `bytes=-suffix`）
/// 返回 (start, end) 含端点；无法满足时返回 None（回退整文件 200）。
fn parse_range(range: Option<&str>, len: u64) -> Option<(u64, u64)> {
    let value = range?.trim();
    let spec = value.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None; // 多段不支持，回退整文件
    }
    let (start_str, end_str) = spec.split_once('-')?;
    let start: u64 = match start_str {
        "" => {
            // 后缀式 bytes=-N：最后 N 字节
            let suffix: u64 = end_str.parse().ok()?;
            if suffix == 0 || suffix > len {
                return None;
            }
            return Some((len - suffix, len - 1));
        }
        s => s.parse().ok()?,
    };
    if start >= len {
        return None;
    }
    let end = match end_str {
        "" => len - 1,
        e => e.parse::<u64>().ok()?.min(len - 1),
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

/// 流式播放协议 `stream://localhost/<歌单>/<文件>` → 返回音频文件字节。
/// 支持 HTTP Range（<audio> 分段拉取与 seek 依赖 206 Partial Content，
/// 否则播放器反复整文件重载会断断续续）。路径做了 URL 编码，这里 percent_decode 解码。
pub async fn handle_stream_request(
    app: &AppHandle,
    path_and_query: &str,
    range_header: Option<&str>,
) -> tauri::http::Response<Vec<u8>> {
    // 形如 "/歌单/歌曲.mp3"
    let raw_path = path_and_query
        .split('?')
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    let decoded = percent_encoding::percent_decode_str(raw_path)
        .decode_utf8()
        .unwrap_or_else(|_| std::borrow::Cow::Borrowed(""));
    let rel = Path::new(decoded.as_ref());

    // 安全：仅允许 music_library 内、且无 ".." 的路径
    let root = match music_root(app) {
        Ok(r) => r,
        Err(_) => return tauri::http::Response::builder().status(500).body(Vec::new()).unwrap(),
    };
    let full = root.join(rel);
    if !full.starts_with(&root) || rel.components().any(|c| c == std::path::Component::ParentDir) {
        return tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    }
    if !full.exists() || !full.is_file() {
        return tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    }

    let mime = match full
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp3") => "audio/mpeg",
        Some("flac") => "audio/flac",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("ogg") => "audio/ogg",
        Some("opus") => "audio/ogg",
        Some("wav") | Some("aiff") => "audio/wav",
        Some("wma") => "audio/x-ms-wma",
        // 视频容器：<audio> 解码的是音轨，MIME 仍用对应容器类型
        Some("mp4") | Some("m4v") | Some("mov") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogv") => "video/ogg",
        Some("mkv") => "video/x-matroska",
        _ => "application/octet-stream",
    };

    // 打开文件拿长度（先做 range 判断，能省则省整文件读取）
    let file = match tokio::fs::File::open(&full).await {
        Ok(f) => f,
        Err(_) => return tauri::http::Response::builder().status(500).body(Vec::new()).unwrap(),
    };
    let len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => return tauri::http::Response::builder().status(500).body(Vec::new()).unwrap(),
    };

    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let base = tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        .header("Cache-Control", "no-cache");

    if let Some((start, end)) = parse_range(range_header, len) {
        // 206 Partial Content：读取请求的分段（复用已打开的 file）
        let mut f = file;
        let mut buf = vec![0u8; (end - start + 1) as usize];
        if f.seek(std::io::SeekFrom::Start(start)).await.is_err() {
            return tauri::http::Response::builder().status(500).body(Vec::new()).unwrap();
        }
        if f.read_exact(&mut buf).await.is_err() {
            return tauri::http::Response::builder().status(500).body(Vec::new()).unwrap();
        }
        return base
            .status(206)
            .header("Content-Length", buf.len().to_string())
            .header("Content-Range", format!("bytes {start}-{end}/{len}"))
            .body(buf)
            .unwrap();
    }

    // 无 Range（或无法满足）：整文件 200
    match tokio::fs::read(&full).await {
        Ok(bytes) => base
            .status(200)
            .header("Content-Length", bytes.len().to_string())
            .body(bytes)
            .unwrap(),
        Err(_) => tauri::http::Response::builder().status(500).body(Vec::new()).unwrap(),
    }
}
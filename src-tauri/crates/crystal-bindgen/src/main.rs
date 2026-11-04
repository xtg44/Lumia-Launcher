//! crystal-bindgen —— 构建辅助工具。
//!
//! 读取已编译的 crystal-core 动态库，生成 Kotlin 绑定（供安卓 Kotlin 壳调用）。
//! 用法：
//!   cargo run -p crystal-bindgen -- <libcrystal_core.dylib/.so 路径> <输出目录>

use anyhow::Context;
use camino::Utf8PathBuf;
use uniffi_bindgen::bindings::{GenerateOptions, TargetLanguage};
use uniffi_bindgen::{BindgenPaths, bindings};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let source: Utf8PathBuf = args
        .next()
        .map(Utf8PathBuf::from)
        .context("缺少参数：动态库路径（如 target/debug/libcrystal_core.dylib）")?;
    let out_dir: Utf8PathBuf = args
        .next()
        .map(Utf8PathBuf::from)
        .context("缺少参数：绑定输出目录")?;

    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("无法创建输出目录 {}", out_dir))?;

    let options = GenerateOptions {
        languages: vec![TargetLanguage::Kotlin],
        source,
        out_dir,
        // try_format：本机不一定装了 ktlint，避免失败
        format: false,
        crate_filter: Some("crystal_core".to_string()),
        // 关键：禁用 cargo-metadata 全量扫描，避免进程卡死
        metadata_no_deps: true,
        ..Default::default()
    };

    bindings::generate_with_bindgen_paths(options, BindgenPaths::default())
        .context("为 crystal-core 生成 Kotlin 绑定失败")?;

    println!("OK: Kotlin bindings for crystal-core generated");
    Ok(())
}
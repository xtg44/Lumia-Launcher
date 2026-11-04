//! 微软账号登录 —— 实现已迁移到 `crates/crystal-core`（平台无关共用）。
//! 本文件仅作重新导出，保证 main.rs 中 `auth::xxx` 调用路径不变（桌面端零回归）。

pub use crystal_core::auth::*;
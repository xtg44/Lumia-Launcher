//! crystal-core —— 平台无关的 Crystal 核心库。
//!
//! 桌面端（Tauri）与安卓端共用同一份源码。这里只放**不依赖 tauri、不假设桌面路径**的纯逻辑：
//! 版本清单解析、BMCLAPI 下载、Mod/整合包处理、Lumi 解释器、auth 设备码登录。
//!
//! 约束：
//! - 不使用 `std::process`
//! - 不使用 tauri 类型
//! - 路径一律由调用方注入，不写死桌面目录

pub mod version;
pub mod auth;
pub mod net;

pub use auth::{
    AuthResult, DeviceCodeResponse, DeviceCodeState, MinecraftAuthResponse, MinecraftProfile,
    TokenResponse,
};
pub use net::{ModSearchResult, modrinth_client, search_modrinth_mods, url_encode};
pub use version::{VersionManifestItem, fetch_versions, fetch_versions_blocking, parse_versions};

/// 版本分类
pub const CATEGORY_RELEASE: &str = "release";
pub const CATEGORY_SNAPSHOT: &str = "snapshot";
pub const CATEGORY_APRIL_FOOL: &str = "april_fool";
pub const CATEGORY_ANCIENT: &str = "ancient";
pub const CATEGORY_UNKNOWN: &str = "unknown";

// ========== FFI 桥接（仅安卓构建时启用） ==========
// 通过 uniffi 暴露给 Kotlin 的入口。feature = "ffi" 时才编译，
// 电脑版（tauri）不启用它，完全不影响桌面端。
#[cfg(feature = "ffi")]
uniffi::setup_scaffolding!();

/// FFI 向 Kotlin 传送错误的统一类型（uniffi 不直接支持 `String` 作 Err）。
#[cfg(feature = "ffi")]
#[derive(Debug, uniffi::Error)]
pub enum CoreError {
    /// 人类可读的错误信息（直接透传给 UI 显示）。
    Message { detail: String },
}

#[cfg(feature = "ffi")]
impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoreError::Message { detail } => write!(f, "{}", detail),
        }
    }
}

/// 将 crystal-core 使用的 `Result<T, String>` 转为 FFI 错误。
#[cfg(feature = "ffi")]
fn to_core_error<T>(res: Result<T, String>) -> Result<T, CoreError> {
    res.map_err(|detail| CoreError::Message { detail })
}

#[cfg(feature = "ffi")]
#[uniffi::export]
pub fn core_url_encode(s: String) -> String {
    net::url_encode(&s)
}

/// 同步拉取 MC 版本清单（BMCLAPI → Mojang 兜底），返回可排序的版本列表。
#[cfg(feature = "ffi")]
#[uniffi::export]
pub fn core_fetch_versions() -> Result<Vec<VersionManifestItem>, CoreError> {
    to_core_error(version::fetch_versions_blocking())
}
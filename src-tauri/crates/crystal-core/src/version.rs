//! MC 版本清单（version_manifest_v2.json）的获取与解析。
//!
//! 数据源为 BMCLAPI 镜像（优先）与 Mojang 官方（兜底），纯 HTTP + JSON，
//! 无任何平台依赖，桌面端与安卓端共用。

use serde::Serialize;

/// 一个在线版本条目。
#[derive(Serialize, Clone, Debug)]
#[cfg_attr(feature = "ffi", derive(uniffi::Record))]
pub struct VersionManifestItem {
    pub id: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    pub category: String,
}

/// 拉取并解析版本清单。返回 `Result<Vec<VersionManifestItem>, String>`，
/// `String` 为人类可读的错误信息，便于两端直接透传给 UI。
///
/// 数据源顺序：BMCLAPI → Mojang 官方。任一成功即返回。
pub async fn fetch_versions() -> Result<Vec<VersionManifestItem>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Crystal-Launcher/1.0")
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

/// 同步拉取版本清单（供 FFI/安卓端使用，uniffi 导出为阻塞调用）。
/// 复用与 `fetch_versions` 完全相同的两端点与解析逻辑。
pub fn fetch_versions_blocking() -> Result<Vec<VersionManifestItem>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Crystal-Launcher/1.0")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))?;

    let endpoints = [
        "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json",
        "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json",
    ];
    let mut errors = Vec::new();

    for endpoint in endpoints {
        match client.get(endpoint).send() {
            Ok(response) if response.status().is_success() => {
                match serde_json::from_str::<serde_json::Value>(
                    &response.text().unwrap_or_default(),
                ) {
                    Ok(json) => return parse_versions(json),
                    Err(error) => errors.push(format!("{}：解析响应失败: {}", endpoint, error)),
                }
            }
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

/// 将 version_manifest_v2.json 的顶层 `{"versions": [...]}` 解析为版本列表。
pub fn parse_versions(json: serde_json::Value) -> Result<Vec<VersionManifestItem>, String> {
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
                "release" => crate::CATEGORY_RELEASE,
                "snapshot" => {
                    if is_april_fool(id) {
                        crate::CATEGORY_APRIL_FOOL
                    } else {
                        crate::CATEGORY_SNAPSHOT
                    }
                }
                "old_beta" | "old_alpha" => crate::CATEGORY_ANCIENT,
                _ => {
                    if is_april_fool(id) {
                        crate::CATEGORY_APRIL_FOOL
                    } else if id.starts_with("b") || id.starts_with("a") || id.starts_with("indev") {
                        crate::CATEGORY_ANCIENT
                    } else {
                        crate::CATEGORY_UNKNOWN
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> serde_json::Value {
        serde_json::json!({
            "versions": [
                { "id": "1.21.4", "type": "release", "releaseTime": "2024-12-03" },
                { "id": "25w14craftmine", "type": "snapshot", "releaseTime": "2025-04-01" },
                { "id": "1.21.3", "type": "release", "releaseTime": "2024-10-23" },
                { "id": "b1.7.3", "type": "old_beta", "releaseTime": "2011-07-08" },
                { "id": "1.21-experimental", "type": "experimental", "releaseTime": "2024-06-01" },
                { "id": "25w01a", "type": "snapshot", "releaseTime": "2025-01-03" }
            ]
        })
    }

    #[test]
    fn parses_release_and_sorts() {
        let list = parse_versions(sample_manifest()).unwrap();
        let release: Vec<_> = list.iter().filter(|v| v.category == "release").collect();
        assert_eq!(release.len(), 2);
        assert_eq!(release[0].id, "1.21.4");
    }

    #[test]
    fn classifies_april_fool_snapshot_as_april_fool() {
        let list = parse_versions(sample_manifest()).unwrap();
        let item = list.iter().find(|v| v.id == "25w14craftmine").unwrap();
        assert_eq!(item.category, "april_fool");
    }

    #[test]
    fn classifies_old_beta_as_ancient() {
        let list = parse_versions(sample_manifest()).unwrap();
        let item = list.iter().find(|v| v.id == "b1.7.3").unwrap();
        assert_eq!(item.category, "ancient");
    }

    #[test]
    fn filters_experimental_versions() {
        let list = parse_versions(sample_manifest()).unwrap();
        assert!(!list.iter().any(|v| v.id.contains("experimental")));
    }

    #[test]
    fn rejects_invalid_manifest() {
        assert!(parse_versions(serde_json::json!({ "foo": [] })).is_err());
    }

    #[test]
    fn preserves_release_time() {
        let list = parse_versions(sample_manifest()).unwrap();
        let item = list.iter().find(|v| v.id == "1.21.3").unwrap();
        assert_eq!(item.release_time, "2024-10-23");
    }
}
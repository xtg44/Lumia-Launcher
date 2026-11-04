//! 网络查询层：URL 编码、Modrinth 模组搜索等平台无关逻辑。
//!
//! 纯 reqwest + serde，无 tauri / 无文件系统依赖。桌面端与安卓端共用。

use serde::Serialize;

/// Mod 搜索统一结果（camelCase 供前端直接消费，也作为安卓端 JSON 结构）。
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModSearchResult {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub icon_url: String,
    pub source: String,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub downloads: u64,
    pub updated_at: String,
}

/// RFC 3986 样式百分号编码（保留 A-Z a-z 0-9 - _ . ~）。
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 构造 Modrinth 请求客户端（统一 UA 与超时）。
pub fn modrinth_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Crystal-Launcher/1.0 (Crystal Launcher; modrinth search)")
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("创建网络客户端失败: {}", e))
}

/// 搜索 Modrinth 模组（纯网络，不依赖任何平台）。
pub async fn search_modrinth_mods(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_encode_reserves_safe_chars() {
        assert_eq!(url_encode("abc-._~"), "abc-._~");
        assert_eq!(url_encode(" "), "%20");
        assert_eq!(url_encode("中文"), "%E4%B8%AD%E6%96%87");
        assert_eq!(url_encode("a/b"), "a%2Fb");
    }

    #[test]
    fn url_encode_via_parser_roundtrip() {
        let s = "hello world/Java Edition";
        let enc = url_encode(s);
        assert!(enc.contains("%20"));
        assert!(enc.contains("%2F"));
        assert!(!enc.contains(' '));
    }

    #[test]
    fn mod_search_result_serializes_camel_case() {
        let r = ModSearchResult {
            id: "abc".into(),
            name: "Sodium".into(),
            summary: "".into(),
            icon_url: "".into(),
            source: "modrinth".into(),
            loaders: vec!["fabric".into()],
            game_versions: vec!["1.20.1".into()],
            downloads: 100,
            updated_at: "2024-01-01".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        // 前端约定 camelCase：gameVersions / iconUrl
        assert!(v.get("gameVersions").is_some());
        assert!(v.get("iconUrl").is_some());
    }
}
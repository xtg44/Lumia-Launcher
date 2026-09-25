use serde::{Deserialize, Serialize};

// Azure 应用 Client ID（需替换为实际注册应用的 ID）
const CLIENT_ID: &str = "01bb9d5d-ae5c-4030-9a1e-c9d7236df173";

// ========== 数据结构 ==========

#[derive(Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
}

#[derive(Deserialize)]
struct XblResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XblDisplayClaims,
}

#[derive(Deserialize)]
struct XblDisplayClaims {
    xui: Vec<XblXui>,
}

#[derive(Deserialize)]
struct XblXui {
    uhs: String,
}

#[derive(Deserialize)]
struct XstsResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XblDisplayClaims,
}

#[derive(Deserialize)]
struct XstsError {
    #[serde(rename = "XErr")]
    xerr: Option<i64>,
    #[serde(rename = "Message")]
    message: Option<String>,
}

#[derive(Deserialize)]
pub struct MinecraftAuthResponse {
    pub access_token: String,
}

#[derive(Deserialize)]
pub struct MinecraftProfile {
    pub id: String,
    pub name: String,
}

// 返回给前端的统一结果
#[derive(Serialize, Clone)]
pub struct AuthResult {
    pub success: bool,
    pub uuid: Option<String>,
    pub username: Option<String>,
    pub access_token: Option<String>,
    pub error: Option<String>,
    pub user_code: Option<String>,
    pub verification_uri: Option<String>,
    pub message: Option<String>,
}

// 设备代码状态（存储在 AppState 中）
#[derive(Clone)]
pub struct DeviceCodeState {
    pub device_code: String,
    pub interval: u64,
    pub expires_at: std::time::Instant,
}

// ========== 步骤 1：获取设备代码 ==========
pub async fn request_device_code(client: &reqwest::Client) -> Result<DeviceCodeResponse, String> {
    let params = [
        ("client_id", CLIENT_ID),
        ("scope", "XboxLive.signin offline_access"),
    ];

    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("请求设备代码失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("获取设备代码失败 (HTTP {}): {}", status, body));
    }

    response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|e| format!("解析设备代码响应失败: {}", e))
}

// ========== 步骤 2：轮询获取 MS Token ==========
pub async fn poll_for_token(
    client: &reqwest::Client,
    device_code: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", CLIENT_ID),
        ("device_code", device_code),
    ];

    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("轮询 token 失败: {}", e))?;

    if response.status().is_success() {
        return response
            .json::<TokenResponse>()
            .await
            .map_err(|e| format!("解析 token 响应失败: {}", e));
    }

    let body = response.text().await.unwrap_or_default();
    if let Ok(err) = serde_json::from_str::<TokenError>(&body) {
        match err.error.as_str() {
            "authorization_pending" => return Err("authorization_pending".to_string()),
            "slow_down" => return Err("slow_down".to_string()),
            "expired_token" => return Err("设备代码已过期，请重新登录".to_string()),
            "access_denied" => return Err("用户取消了授权".to_string()),
            _ => {}
        }
    }

    Err(format!("获取 token 失败: {}", body))
}

// ========== 步骤 3：MS Token → XBL Token ==========
pub async fn authenticate_xbl(
    client: &reqwest::Client,
    ms_token: &str,
) -> Result<(String, String), String> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={}", ms_token)
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });

    let response = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("XBL 认证请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("XBL 认证失败 (HTTP {}): {}", status, body));
    }

    let xbl: XblResponse = response
        .json()
        .await
        .map_err(|e| format!("解析 XBL 响应失败: {}", e))?;

    let uhs = xbl
        .display_claims
        .xui
        .first()
        .ok_or("XBL 响应缺少用户哈希")?
        .uhs
        .clone();

    Ok((xbl.token, uhs))
}

// ========== 步骤 4：XBL Token → XSTS Token ==========
pub async fn authenticate_xsts(
    client: &reqwest::Client,
    xbl_token: &str,
) -> Result<(String, String), String> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl_token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });

    let response = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("XSTS 认证请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body_text = response.text().await.unwrap_or_default();

        if let Ok(err) = serde_json::from_str::<XstsError>(&body_text) {
            if let Some(xerr) = err.xerr {
                match xerr {
                    2148916233 => return Err("此账户没有 Xbox Live 账户，请先创建".to_string()),
                    2148916238 => return Err("此账户是儿童账户，需要家长授权才能登录".to_string()),
                    _ => return Err(format!("XSTS 认证失败 (0x{:x}): {}", xerr, err.message.unwrap_or_default())),
                }
            }
        }

        return Err(format!("XSTS 认证失败 (HTTP {}): {}", status, body_text));
    }

    let xsts: XstsResponse = response
        .json()
        .await
        .map_err(|e| format!("解析 XSTS 响应失败: {}", e))?;

    let uhs = xsts
        .display_claims
        .xui
        .first()
        .ok_or("XSTS 响应缺少用户哈希")?
        .uhs
        .clone();

    Ok((xsts.token, uhs))
}

// ========== 步骤 5：XSTS Token → Minecraft Token ==========
pub async fn authenticate_minecraft(
    client: &reqwest::Client,
    uhs: &str,
    xsts_token: &str,
) -> Result<MinecraftAuthResponse, String> {
    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={};{}", uhs, xsts_token)
    });

    let response = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Minecraft 认证请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        if status == 403 {
            return Err("此 Microsoft 账户未拥有 Minecraft，请先购买游戏".to_string());
        }
        return Err(format!("Minecraft 认证失败 (HTTP {}): {}", status, body));
    }

    response
        .json::<MinecraftAuthResponse>()
        .await
        .map_err(|e| format!("解析 Minecraft 认证响应失败: {}", e))
}

// ========== 步骤 6：获取玩家信息 ==========
pub async fn get_minecraft_profile(
    client: &reqwest::Client,
    mc_token: &str,
) -> Result<MinecraftProfile, String> {
    let response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .header("Authorization", format!("Bearer {}", mc_token))
        .send()
        .await
        .map_err(|e| format!("获取玩家信息请求失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        if status == 404 {
            return Err("此 Microsoft 账户未拥有 Minecraft，请先购买游戏".to_string());
        }
        let body = response.text().await.unwrap_or_default();
        return Err(format!("获取玩家信息失败 (HTTP {}): {}", status, body));
    }

    response
        .json::<MinecraftProfile>()
        .await
        .map_err(|e| format!("解析玩家信息响应失败: {}", e))
}

// ========== 完整认证流程（步骤 3-6）==========
pub async fn ms_token_to_mc_profile(
    client: &reqwest::Client,
    ms_access_token: &str,
) -> Result<(MinecraftProfile, MinecraftAuthResponse), String> {
    let (xbl_token, _xbl_uhs) = authenticate_xbl(client, ms_access_token).await?;
    let (xsts_token, xsts_uhs) = authenticate_xsts(client, &xbl_token).await?;
    let mc_auth = authenticate_minecraft(client, &xsts_uhs, &xsts_token).await?;
    let profile = get_minecraft_profile(client, &mc_auth.access_token).await?;
    Ok((profile, mc_auth))
}

// ========== 刷新 MS Token ==========
pub async fn refresh_access_token(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
        ("scope", "XboxLive.signin offline_access"),
    ];

    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("刷新 token 失败: {}", e))?;

    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("刷新 token 失败 (HTTP {}): {}", status, body));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| format!("解析刷新 token 响应失败: {}", e))
}

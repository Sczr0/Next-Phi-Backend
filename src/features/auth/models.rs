use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Debug, Deserialize)]
pub struct Wrap<T> {
    pub success: bool,
    pub data: T,
}

#[derive(Debug, Deserialize, Clone, ToSchema)]
pub struct Token {
    pub kid: String,
    pub mac_key: String,
}

#[derive(Debug, Deserialize, Clone, ToSchema)]
pub struct Account {
    pub openid: String,
    pub unionid: String,
}

#[derive(Debug, Deserialize, Clone, ToSchema)]
pub struct DeviceCodeResponse {
    pub device_code: Option<String>,
    pub verification_url: Option<String>,
    pub user_code: Option<String>,
    pub interval: Option<u64>,
    pub expires_in: Option<u64>,
    pub qrcode_url: Option<String>,
}

#[derive(Debug, Deserialize, Clone, ToSchema)]
pub struct SessionData {
    /// LeanCloud Session Token
    #[schema(example = "r:abcdefg.hijklmn-opqrstuvwxyz")]
    pub session_token: String,
}

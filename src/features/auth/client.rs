use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::Mac;
use rand::RngCore;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::error::AppError;

use super::models::{Account, DeviceCodeResponse, SessionData, Token, Wrap};

const LC_APP_ID: &str = "rAK3FfdieFob2Nn8Am";
const LC_APP_KEY: &str = "Qr9AEqtuoSVS3zeD6iVbM4ZC0AtkJcQ89tywVyi0";

#[derive(Clone)]
pub struct TapTapClient {
    pub client: reqwest::Client,
    tap_headers: HeaderMap,
    phi_headers: HeaderMap,
}

impl TapTapClient {
    pub fn new() -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .http1_title_case_headers()
            .user_agent("TapTapUnitySDK/1.0 UnityPlayer/2021.3.40f1c1")
            .build()
            .map_err(|e| AppError::Internal(format!("初始化 HTTP Client 失败: {}", e)))?;

        let mut tap_headers = HeaderMap::new();
        tap_headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        tap_headers.insert(
            "User-Agent",
            HeaderValue::from_static("TapTapAndroidSDK/3.16.5"),
        );

        let mut phi_headers = HeaderMap::new();
        phi_headers.insert(
            "User-Agent",
            HeaderValue::from_static("LeanCloud-CSharp-SDK/1.0.3"),
        );
        phi_headers.insert("X-LC-Id", HeaderValue::from_static(LC_APP_ID));
        phi_headers.insert("X-LC-Key", HeaderValue::from_static(LC_APP_KEY));
        phi_headers.insert("Content-Type", HeaderValue::from_static("application/json"));

        Ok(Self {
            client,
            tap_headers,
            phi_headers,
        })
    }

    pub async fn request_device_code(
        &self,
        device_id: &str,
    ) -> Result<DeviceCodeResponse, AppError> {
        let info = serde_json::json!({ "device_id": device_id }).to_string();

        let form = [
            ("client_id", LC_APP_ID),
            ("response_type", "device_code"),
            ("scope", "basic_info"),
            ("version", "1.2.0"),
            ("platform", "unity"),
            ("info", &info),
        ];

        let resp = self
            .client
            .post("https://www.taptap.com/oauth2/v1/device/code")
            .headers(self.tap_headers.clone())
            .form(&form)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("设备码请求失败: {}", e)))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Json(format!("解析设备码响应失败: {}", e)))?;

        let success = body
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            let detail = body.get("data").cloned().unwrap_or(Value::Null);
            return Err(AppError::Auth(format!("TapTap 业务错误: {}", detail)));
        }

        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let parsed: DeviceCodeResponse = serde_json::from_value(data)
            .map_err(|e| AppError::Json(format!("设备码数据解析失败: {}", e)))?;
        Ok(parsed)
    }

    pub async fn poll_for_token(
        &self,
        device_code: &str,
        device_id: &str,
    ) -> Result<SessionData, AppError> {
        // 交换 token
        let info = serde_json::json!({ "device_id": device_id }).to_string();
        let form = [
            ("grant_type", "device_token"),
            ("client_id", LC_APP_ID),
            ("secret_type", "hmac-sha-1"),
            ("code", device_code),
            ("version", "1.0"),
            ("platform", "unity"),
            ("info", &info),
        ];

        let resp = self
            .client
            .post("https://www.taptap.cn/oauth2/v1/token")
            .headers(self.tap_headers.clone())
            .form(&form)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("获取 Token 失败: {}", e)))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| AppError::Json(format!("解析 Token 响应失败: {}", e)))?;

        let success = body
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            let data = body.get("data").cloned().unwrap_or(Value::Null);
            // 尝试提取标准的 OAuth 设备码错误码
            let (code, message) = if let Some(obj) = data.as_object() {
                let code = obj
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let message = obj
                    .get("error_description")
                    .or_else(|| obj.get("msg"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (code, message)
            } else if let Some(s) = data.as_str() {
                (String::new(), s.to_string())
            } else {
                (String::new(), data.to_string())
            };

            let msg = if message.is_empty() {
                format!("TapTap 业务错误: {}", data)
            } else {
                message
            };

            let code_l = code.to_ascii_lowercase();
            if code_l.contains("authorization_pending") || code_l.contains("slow_down") {
                return Err(AppError::AuthPending(msg));
            } else {
                return Err(AppError::Auth(msg));
            }
        }

        let token_val = body.get("data").cloned().unwrap_or(Value::Null);
        let token: Token = serde_json::from_value(token_val)
            .map_err(|e| AppError::Json(format!("Token 数据解析失败: {}", e)))?;

        // 查询基本信息
        let auth_header = self.build_mac_authorization(&token)?;
        let account_resp = self
            .client
            .get(&format!(
                "https://open.tapapis.cn/account/basic-info/v1?client_id={}",
                LC_APP_ID
            ))
            .headers(self.tap_headers.clone())
            .header("Authorization", auth_header)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("获取账号信息失败: {}", e)))?;

        let account_wrap: Wrap<Account> = account_resp
            .json()
            .await
            .map_err(|e| AppError::Json(format!("解析账号信息失败: {}", e)))?;
        let account = account_wrap.data;

        // 通过 LeanCloud 创建/登录用户，返回 SessionToken
        let auth_data = serde_json::json!({
            "authData": {
                "taptap": {
                    "kid": token.kid,
                    "access_token": token.kid,
                    "token_type": "mac",
                    "mac_key": token.mac_key,
                    "mac_algorithm": "hmac-sha-1",
                    "openid": account.openid,
                    "unionid": account.unionid,
                }
            }
        });

        let lc_resp = self
            .client
            .post("https://rak3ffdi.cloud.tds1.tapapis.cn/1.1/users")
            .headers(self.phi_headers.clone())
            .json(&auth_data)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("请求 LeanCloud 失败: {}", e)))?;

        let status = lc_resp.status();
        if !status.is_success() {
            let text = lc_resp
                .text()
                .await
                .unwrap_or_else(|_| "<body 读取失败>".to_string());
            return Err(AppError::Auth(format!(
                "LeanCloud 认证失败: HTTP {} - {}",
                text, status
            )));
        }

        #[derive(serde::Deserialize)]
        struct LcUserResp {
            #[serde(rename = "sessionToken")]
            session_token: String,
        }

        let user: LcUserResp = lc_resp
            .json()
            .await
            .map_err(|e| AppError::Json(format!("解析 LeanCloud 响应失败: {}", e)))?;

        Ok(SessionData {
            session_token: user.session_token,
        })
    }

    fn build_mac_authorization(&self, token: &Token) -> Result<String, AppError> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AppError::Internal(format!("时间计算失败: {}", e)))?
            .as_secs();

        let mut rng = rand::thread_rng();
        let nonce: u32 = rng.next_u32();

        let input = format!(
            "{}\n{}\nGET\n/account/basic-info/v1?client_id={}\nopen.tapapis.cn\n443\n\n",
            ts, nonce, LC_APP_ID
        );

        let mut mac = hmac::Hmac::<sha1::Sha1>::new_from_slice(token.mac_key.as_bytes())
            .map_err(|e| AppError::Internal(format!("HMAC 初始化失败: {}", e)))?;
        mac.update(input.as_bytes());
        let mac = base64::prelude::BASE64_STANDARD.encode(mac.finalize().into_bytes());
        let header = format!(
            "MAC id=\"{}\",ts=\"{}\",nonce=\"{}\",mac=\"{}\"",
            token.kid, ts, nonce, mac
        );
        Ok(header)
    }
}

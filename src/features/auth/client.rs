use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::Mac;
use rand::RngCore;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::error::AppError;

use super::models::{Account, DeviceCodeResponse, SessionData, Token, Wrap};

use crate::config::{TapTapConfig, TapTapMultiConfig, TapTapVersion};

#[derive(Clone)]
pub struct TapTapClient {
    pub client: reqwest::Client,
    tap_headers: HeaderMap,
    phi_headers: HeaderMap,
    config: TapTapMultiConfig,
}

impl TapTapClient {
    pub fn new(config: &TapTapMultiConfig) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .http1_title_case_headers()
            .user_agent("TapTapUnitySDK/1.0 UnityPlayer/2021.3.40f1c1")
            .build()
            .map_err(|e| AppError::Internal(format!("初始化 HTTP Client 失败: {e}")))?;

        let mut tap_headers = HeaderMap::new();
        tap_headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        tap_headers.insert(
            "User-Agent",
            HeaderValue::from_static("TapTapAndroidSDK/3.16.5"),
        );

        // 使用大陆版配置初始化phi_headers，后续会根据请求动态调整
        let cn_config = &config.cn;
        let mut phi_headers = HeaderMap::new();
        phi_headers.insert(
            "User-Agent",
            HeaderValue::from_static("LeanCloud-CSharp-SDK/1.0.3"),
        );
        phi_headers.insert(
            "X-LC-Id",
            HeaderValue::from_str(&cn_config.leancloud_app_id)
                .map_err(|e| AppError::Internal(format!("无效的 Header 值: {e}")))?,
        );
        phi_headers.insert(
            "X-LC-Key",
            HeaderValue::from_str(&cn_config.leancloud_app_key)
                .map_err(|e| AppError::Internal(format!("无效的 Header 值: {e}")))?,
        );
        phi_headers.insert("Content-Type", HeaderValue::from_static("application/json"));

        Ok(Self {
            client,
            tap_headers,
            phi_headers,
            config: config.clone(),
        })
    }

    /// 根据版本获取对应的配置
    fn get_config(&self, version: Option<&str>) -> &TapTapConfig {
        match version {
            Some("global") => &self.config.global,
            Some("cn") => &self.config.cn,
            None => match self.config.default_version {
                TapTapVersion::CN => &self.config.cn,
                TapTapVersion::Global => &self.config.global,
            },
            _ => match self.config.default_version {
                TapTapVersion::CN => &self.config.cn,
                TapTapVersion::Global => &self.config.global,
            },
        }
    }

    pub async fn request_device_code(
        &self,
        device_id: &str,
        version: Option<&str>,
    ) -> Result<DeviceCodeResponse, AppError> {
        let info = serde_json::json!({ "device_id": device_id }).to_string();
        let config = self.get_config(version);

        let form = [
            ("client_id", config.leancloud_app_id.as_str()),
            ("response_type", "device_code"),
            ("scope", "basic_info"),
            ("version", "1.2.0"),
            ("platform", "unity"),
            ("info", info.as_str()),
        ];

        let resp = self
            .client
            .post(&config.device_code_endpoint)
            .headers(self.tap_headers.clone())
            .form(&form)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("设备码请求失败: {e}")))?;

        let status = resp.status();
        let body_text = resp
            .text()
            .await
            .map_err(|e| AppError::Network(format!("读取设备码响应体失败: {e}")))?;

        if !status.is_success() {
            tracing::warn!("TapTap 设备码请求失败：HTTP {status}");
            return Err(AppError::Network(format!("TapTap 设备码请求失败: HTTP {status}")));
        }

        let body: Value = serde_json::from_str(&body_text)
            .map_err(|e| AppError::Network(format!("TapTap 设备码响应解析失败: {e}")))?;

        let success = body
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !success {
            let data = body.get("data").cloned().unwrap_or(Value::Null);
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
                (String::new(), String::new())
            };

            let msg = if !message.trim().is_empty() {
                format!("TapTap 设备码申请失败: {message}")
            } else if !code.trim().is_empty() {
                format!("TapTap 设备码申请失败: {code}")
            } else {
                "TapTap 设备码申请失败".to_string()
            };
            return Err(AppError::Auth(msg));
        }

        let data = body.get("data").cloned().unwrap_or(Value::Null);
        let parsed: DeviceCodeResponse = serde_json::from_value(data)
            .map_err(|e| AppError::Network(format!("TapTap 设备码数据解析失败: {e}")))?;
        Ok(parsed)
    }

    pub async fn poll_for_token(
        &self,
        device_code: &str,
        device_id: &str,
        version: Option<&str>,
    ) -> Result<SessionData, AppError> {
        // 交换 token
        let info = serde_json::json!({ "device_id": device_id }).to_string();
        let config = self.get_config(version);

        let form = [
            ("grant_type", "device_token"),
            ("client_id", config.leancloud_app_id.as_str()),
            ("secret_type", "hmac-sha-1"),
            ("code", device_code),
            ("version", "1.0"),
            ("platform", "unity"),
            ("info", info.as_str()),
        ];

        let resp = self
            .client
            .post(&config.token_endpoint)
            .headers(self.tap_headers.clone())
            .form(&form)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("获取 Token 失败: {e}")))?;

        let status = resp.status();
        let body_text = resp
            .text()
            .await
            .map_err(|e| AppError::Network(format!("读取 Token 响应体失败: {e}")))?;
        if !status.is_success() {
            tracing::warn!("TapTap token 请求失败：HTTP {status}");
            return Err(AppError::Network(format!("TapTap token 请求失败: HTTP {status}")));
        }
        let body: Value = serde_json::from_str(&body_text)
            .map_err(|e| AppError::Network(format!("TapTap token 响应解析失败: {e}")))?;

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

            let msg = if !message.trim().is_empty() {
                message
            } else if !code.trim().is_empty() {
                format!("TapTap 业务错误: {code}")
            } else {
                "TapTap 业务错误".to_string()
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
            .map_err(|e| AppError::Network(format!("TapTap token 数据解析失败: {e}")))?;

        // 查询基本信息
        let auth_header = self.build_mac_authorization(&token, config.leancloud_app_id.as_str())?;
        let account_resp = self
            .client
            .get(format!(
                "{}?client_id={}",
                config.user_info_endpoint, config.leancloud_app_id
            ))
            .headers(self.tap_headers.clone())
            .header("Authorization", auth_header)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("获取账号信息失败: {e}")))?;

        let status = account_resp.status();
        let body_text = account_resp
            .text()
            .await
            .map_err(|e| AppError::Network(format!("读取账号信息响应体失败: {e}")))?;
        if !status.is_success() {
            tracing::warn!("TapTap 账号信息请求失败：HTTP {status}");
            return Err(AppError::Network(format!("TapTap 账号信息请求失败: HTTP {status}")));
        }
        let account_wrap: Wrap<Account> = serde_json::from_str(&body_text)
            .map_err(|e| AppError::Network(format!("TapTap 账号信息响应解析失败: {e}")))?;
        if !account_wrap.success {
            return Err(AppError::Auth("TapTap 获取账号信息失败".to_string()));
        }
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

        // 动态调整phi_headers的X-LC-Id和X-LC-Key
        let mut phi_headers = self.phi_headers.clone();
        phi_headers.insert(
            "X-LC-Id",
            HeaderValue::from_str(&config.leancloud_app_id)
                .map_err(|e| AppError::Internal(format!("无效的 Header 值: {e}")))?,
        );
        phi_headers.insert(
            "X-LC-Key",
            HeaderValue::from_str(&config.leancloud_app_key)
                .map_err(|e| AppError::Internal(format!("无效的 Header 值: {e}")))?,
        );

        let lc_resp = self
            .client
            .post(format!("{}/users", config.leancloud_base_url))
            .headers(phi_headers)
            .json(&auth_data)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("请求 LeanCloud 失败: {e}")))?;

        let status = lc_resp.status();
        if !status.is_success() {
            tracing::warn!("LeanCloud 认证失败：HTTP {status}");
            return Err(AppError::Auth(format!("LeanCloud 认证失败: HTTP {status}")));
        }

        #[derive(serde::Deserialize)]
        struct LcUserResp {
            #[serde(rename = "sessionToken")]
            session_token: String,
        }

        let user: LcUserResp = lc_resp
            .json()
            .await
            .map_err(|e| AppError::Network(format!("解析 LeanCloud 响应失败: {e}")))?;

        Ok(SessionData {
            session_token: user.session_token,
        })
    }

    fn build_mac_authorization(
        &self,
        token: &Token,
        leancloud_app_id: &str,
    ) -> Result<String, AppError> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| AppError::Internal(format!("时间计算失败: {e}")))?
            .as_secs();

        let mut rng = rand::thread_rng();
        let nonce: u32 = rng.next_u32();

        let input = format!(
            "{ts}\n{nonce}\nGET\n/account/basic-info/v1?client_id={leancloud_app_id}\nopen.tapapis.cn\n443\n\n"
        );

        let mut mac = hmac::Hmac::<sha1::Sha1>::new_from_slice(token.mac_key.as_bytes())
            .map_err(|e| AppError::Internal(format!("HMAC 初始化失败: {e}")))?;
        mac.update(input.as_bytes());
        let mac = base64::prelude::BASE64_STANDARD.encode(mac.finalize().into_bytes());
        let header = format!(
            "MAC id=\"{}\",ts=\"{}\",nonce=\"{}\",mac=\"{}\"",
            token.kid, ts, nonce, mac
        );
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::TapTapClient;
    use crate::config::{TapTapConfig, TapTapMultiConfig, TapTapVersion};

    fn dummy_cfg(default_version: TapTapVersion) -> TapTapMultiConfig {
        TapTapMultiConfig {
            cn: TapTapConfig {
                device_code_endpoint: "http://example.invalid/device/code".to_string(),
                token_endpoint: "http://example.invalid/token".to_string(),
                user_info_endpoint: "http://example.invalid/userinfo".to_string(),
                leancloud_base_url: "http://example.invalid/leancloud".to_string(),
                leancloud_app_id: "cn-app-id".to_string(),
                leancloud_app_key: "cn-app-key".to_string(),
            },
            global: TapTapConfig {
                device_code_endpoint: "http://example.invalid/device/code".to_string(),
                token_endpoint: "http://example.invalid/token".to_string(),
                user_info_endpoint: "http://example.invalid/userinfo".to_string(),
                leancloud_base_url: "http://example.invalid/leancloud".to_string(),
                leancloud_app_id: "global-app-id".to_string(),
                leancloud_app_key: "global-app-key".to_string(),
            },
            default_version,
        }
    }

    #[test]
    fn get_config_uses_default_version_when_none() {
        let cfg = dummy_cfg(TapTapVersion::Global);
        let client = TapTapClient::new(&cfg).expect("TapTapClient::new");
        let picked = client.get_config(None);
        assert_eq!(picked.leancloud_app_id, "global-app-id");
    }

    #[test]
    fn get_config_prefers_explicit_version_over_default() {
        let cfg = dummy_cfg(TapTapVersion::Global);
        let client = TapTapClient::new(&cfg).expect("TapTapClient::new");
        let picked = client.get_config(Some("cn"));
        assert_eq!(picked.leancloud_app_id, "cn-app-id");
    }
}

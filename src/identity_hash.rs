use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::auth_contract::UnifiedSaveRequest;

pub fn hmac_hex16(salt: &str, value: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(salt.as_bytes()).expect("HMAC key");
    mac.update(value.as_bytes());
    let bytes = mac.finalize().into_bytes();
    hex::encode(&bytes[..16])
}

pub fn derive_user_identity_from_auth(
    salt_opt: Option<&str>,
    auth: &UnifiedSaveRequest,
) -> (Option<String>, Option<String>) {
    let Some(salt) = salt_opt else {
        return (None, None);
    };
    if let Some(tok) = &auth.session_token
        && !tok.is_empty()
    {
        return (
            Some(hmac_hex16(salt, tok)),
            Some("session_token".to_string()),
        );
    }
    if let Some(ext) = &auth.external_credentials {
        if let Some(id) = &ext.api_user_id
            && !id.is_empty()
        {
            return (
                Some(hmac_hex16(salt, id)),
                Some("external_api_user_id".to_string()),
            );
        }
        if let Some(st) = &ext.sessiontoken
            && !st.is_empty()
        {
            return (
                Some(hmac_hex16(salt, st)),
                Some("external_sessiontoken".to_string()),
            );
        }
        if let (Some(p), Some(pid)) = (&ext.platform, &ext.platform_id)
            && !p.is_empty()
            && !pid.is_empty()
        {
            let k = format!("{p}:{pid}");
            return (
                Some(hmac_hex16(salt, &k)),
                Some("platform_pair".to_string()),
            );
        }
    }
    (None, None)
}

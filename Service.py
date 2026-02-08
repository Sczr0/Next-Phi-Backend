import hmac
import base64
import json
import time
import random
from typing import Dict, Any
import httpx
import urllib.parse

# ========== 配置 ==========
TAPTAP_CLIENT_ID = "rAK3FfdieFob2Nn8Am"
DEVICE_CODE_URL = "https://www.taptap.com/oauth2/v1/device/code"
TOKEN_URL = "https://www.taptap.cn/oauth2/v1/token"
ACCOUNT_INFO_URL = "https://open.tapapis.cn/account/basic-info/v1"

LEAN_CLOUD_URL = "https://rak3ffdi.cloud.tds1.tapapis.cn/1.1/users"
LEAN_APP_ID = "rAK3FfdieFob2Nn8Am"
LEAN_APP_KEY = "Qr9AEqtuoSVS3zeD6iVbM4ZC0AtkJcQ89tywVyi0"

HEADERS = {
    "User-Agent": "TapTapAndroidSDK/3.16.5",
    "Content-Type": "application/x-www-form-urlencoded"
}


# ========== 工具函数：生成编码后的 info 参数 ==========
def encode_info(device_id: str) -> str:
    return json.dumps({"device_id": device_id}, separators=(',', ':'))


# ========== 步骤 1: 获取 device_code ==========
async def request_device_code(device_id: str) -> Dict[str, Any]:
    payload = {
        "client_id": TAPTAP_CLIENT_ID,
        "response_type": "device_code",
        "scope": "basic_info",
        "version": "1.2.0",
        "platform": "unity",
        "info": encode_info(device_id),
    }

    async with httpx.AsyncClient() as client:
        resp = await client.post(DEVICE_CODE_URL, data=payload, headers=HEADERS)
        resp.raise_for_status()
        data = resp.json()
        if not data.get("success", True):
            raise RuntimeError(f"Device code error: {data}")
        return data["data"]


# ========== 步骤 2: 换取 token ==========
async def exchange_token(device_code: str, device_id: str) -> Dict[str, Any]:
    payload = {
        "grant_type": "device_token",
        "client_id": TAPTAP_CLIENT_ID,
        "secret_type": "hmac-sha-1",
        "code": device_code,
        "version": "1.0",
        "platform": "unity",
        "info": encode_info(device_id),
    }
    print(payload)

    async with httpx.AsyncClient() as client:
        resp = await client.post(TOKEN_URL, data=payload, headers=HEADERS)
        
        # 先读取 Body，方便调试错误
        try:
            data = resp.json()
        except Exception:
            data = {"raw": resp.text}

        # 如果状态码不是 2xx，手动抛出带 Body 的异常
        if resp.is_error:
            raise RuntimeError(f"HTTP {resp.status_code}: {data}")
            
        if not data.get("success"):
            # 兼容 TapTap 有时返回 200 但 success=false 的情况
            raise RuntimeError(f"Business Error: {data}")
            
        return data["data"]


# ========== 步骤 3: 生成 MAC 鉴权头 ==========
def generate_mac_auth_header(token: Dict[str, str]) -> str:
    ts = int(time.time())
    nonce = random.randint(0, 2 ** 32 - 1)

    input_str = (
        f"{ts}\n{nonce}\nGET\n/account/basic-info/v1?client_id={TAPTAP_CLIENT_ID}\n"
        f"open.tapapis.cn\n443\n\n"
    )

    mac_digest = hmac.new(
        token["mac_key"].encode(),
        input_str.encode(),
        digestmod="sha1"
    ).digest()

    mac_b64 = base64.b64encode(mac_digest).decode()
    return f'MAC id="{token["kid"]}",ts="{ts}",nonce="{nonce}",mac="{mac_b64}"'


# ========== 步骤 4: 获取账户信息 ==========
async def fetch_account_info(token: Dict[str, str]) -> Dict[str, Any]:
    headers = {
        "User-Agent": "TapTapAndroidSDK/3.16.5",
        "Authorization": generate_mac_auth_header(token),
    }
    params = {"client_id": TAPTAP_CLIENT_ID}

    async with httpx.AsyncClient() as client:
        resp = await client.get(ACCOUNT_INFO_URL, params=params, headers=headers)
        resp.raise_for_status()
        data = resp.json()
        return data["data"]


# ========== 步骤 5: 注册到 LeanCloud ==========
async def register_or_login_leancloud(
        openid: str,
        unionid: str,
        token: Dict[str, str]
) -> None:
    headers = {
        "X-LC-Id": LEAN_APP_ID,
        "X-LC-Key": LEAN_APP_KEY,
        "Content-Type": "application/json",
        "User-Agent": "LeanCloud-CSharp-SDK/1.0.3",
    }

    payload = {
        "authData": {
            "taptap": {
                "kid": token["kid"],
                "access_token": token["kid"],
                "token_type": "mac",
                "mac_key": token["mac_key"],
                "mac_algorithm": "hmac-sha-1",
                "openid": openid,
                "unionid": unionid,
            }
        }
    }

    async with httpx.AsyncClient() as client:
        resp = await client.post(LEAN_CLOUD_URL, json=payload, headers=headers)
        resp.raise_for_status()

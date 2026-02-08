import asyncio
import uuid
from Service import request_device_code, exchange_token

async def main():
    # 1. 生成或指定一个固定的 device_id
    device_id = str(uuid.uuid4())
    print(f"Device ID: {device_id}")

    try:
        # 2. 申请设备码
        print("\n[1/3] Requesting Device Code...")
        data = await request_device_code(device_id)
        
        device_code = data['device_code']
        user_code = data['user_code']
        verification_url = data['verification_url']
        interval = data.get('interval', 5)
        
        print(f"Device Code: {device_code}")
        print(f"User Code:   {user_code}")
        
        # 修复链接拼接：必须包含 user_code
        full_url = verification_url
        params = []
        if '?' not in full_url:
            full_url += "?"
        else:
            if not full_url.endswith('?') and not full_url.endswith('&'):
                full_url += "&"

        # 如果原链接里没有 user_code，手动拼上去
        if 'user_code=' not in full_url:
            params.append(f"user_code={user_code}")
        
        params.append("qrcode=1")
        
        full_url += "&".join(params)
            
        print(f"\n>>> 请在浏览器打开以下链接进行授权: \n{full_url}")
        print(f"\n(Waiting for authorization, polling every {interval} seconds...)")

        # 3. 轮询 Token
        print("\n[2/3] Polling for Token...")
        while True:
            try:
                token_data = await exchange_token(device_code, device_id)
                print("\n[SUCCESS] Token Received!")
                print(f"Kid: {token_data.get('kid')}")
                print(f"Mac Key: {token_data.get('mac_key')}")
                break
            except RuntimeError as e:
                msg = str(e).lower()
                # 检查是否是 pending 状态 (兼容 authorization_pending 和 authorization_waiting)
                if "authorization_pending" in msg or "authorization_waiting" in msg:
                    print(".", end="", flush=True)
                    await asyncio.sleep(interval)
                    continue
                elif "slow_down" in msg:
                    print("!", end="", flush=True)
                    await asyncio.sleep(interval + 2)
                    continue
                else:
                    # 真正的错误
                    print(f"\n[ERROR] {msg}")
                    break
            except Exception as e:
                print(f"\n[EXCEPTION] {e}")
                break

    except Exception as e:
        print(f"\n[FATAL ERROR] {e}")

if __name__ == "__main__":
    asyncio.run(main())

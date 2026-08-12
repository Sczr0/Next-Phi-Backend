/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { QrCodeCreateResponse } from '../models/QrCodeCreateResponse';
import type { QrCodeStatusResponse } from '../models/QrCodeStatusResponse';
import type { SessionExchangeRequest } from '../models/SessionExchangeRequest';
import type { SessionExchangeResponse } from '../models/SessionExchangeResponse';
import type { SessionLogoutRequest } from '../models/SessionLogoutRequest';
import type { SessionLogoutResponse } from '../models/SessionLogoutResponse';
import type { UnifiedSaveRequest } from '../models/UnifiedSaveRequest';
import type { UserIdResponse } from '../models/UserIdResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class AuthService {
    /**
     * 生成登录二维码
     * 为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。响应带 Cache-Control: no-store。
     * @returns QrCodeCreateResponse 生成二维码成功
     * @throws ApiError
     */
    public static postQrcode({
        taptapVersion,
    }: {
        /**
         * TapTap 版本：cn 或 global
         */
        taptapVersion?: string,
    }): CancelablePromise<QrCodeCreateResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/auth/qrcode',
            query: {
                'taptapVersion': taptapVersion,
            },
            errors: {
                401: `认证失败（TapTap 返回认证错误）`,
                422: `参数校验失败（taptapVersion 非法等）`,
                500: `服务器内部错误`,
                502: `上游网络错误`,
                504: `上游请求超时`,
            },
        });
    }
    /**
     * 轮询二维码授权状态
     * 根据 qr_id 查询当前授权进度。若返回 Pending 且包含 retry_after，客户端应按该秒数后再轮询。响应带 Cache-Control: no-store。
     * @returns QrCodeStatusResponse 状态返回
     * @throws ApiError
     */
    public static getQrcodeStatus({
        qrId,
    }: {
        /**
         * 二维码ID
         */
        qrId: string,
    }): CancelablePromise<QrCodeStatusResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/auth/qrcode/{qr_id}/status',
            path: {
                'qr_id': qrId,
            },
        });
    }
    /**
     * 签发后端会话令牌
     * 使用登录凭证交换后端短期 access token。
     * @returns SessionExchangeResponse 签发成功
     * @throws ApiError
     */
    public static postSessionExchange({
        xExchangeSecret,
        requestBody,
    }: {
        /**
         * Next.js 与后端共享密钥
         */
        xExchangeSecret: string,
        requestBody: SessionExchangeRequest,
    }): CancelablePromise<SessionExchangeResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/auth/session/exchange',
            headers: {
                'X-Exchange-Secret': xExchangeSecret,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `共享密钥无效或无法识别用户`,
                403: `用户已被封禁`,
                422: `凭证无效`,
                500: `服务端配置错误`,
            },
        });
    }
    /**
     * 注销会话令牌
     * scope=current 仅注销当前令牌，scope=all 注销该用户所有历史令牌。
     * @returns SessionLogoutResponse 注销成功
     * @throws ApiError
     */
    public static postSessionLogout({
        authorization,
        requestBody,
    }: {
        /**
         * Bearer access token
         */
        authorization: string,
        requestBody: SessionLogoutRequest,
    }): CancelablePromise<SessionLogoutResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/auth/session/logout',
            headers: {
                'Authorization': authorization,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `令牌无效`,
                403: `用户已被封禁`,
                422: `请求体 JSON 无效或缺少必填的 scope`,
                500: `存储不可用或配置错误`,
            },
        });
    }
    /**
     * 刷新会话令牌
     * 使用旧的 Bearer access token（允许过期）与 X-Exchange-Secret 换取新的短期 access token。
     * @returns SessionExchangeResponse 刷新成功
     * @throws ApiError
     */
    public static postSessionRefresh({
        authorization,
        xExchangeSecret,
    }: {
        /**
         * Bearer access token（可过期）
         */
        authorization: string,
        /**
         * Next.js 与后端共享密钥
         */
        xExchangeSecret: string,
    }): CancelablePromise<SessionExchangeResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/auth/session/refresh',
            headers: {
                'Authorization': authorization,
                'X-Exchange-Secret': xExchangeSecret,
            },
            errors: {
                401: `共享密钥无效、令牌无效或已撤销`,
                403: `用户已被封禁`,
                500: `存储不可用或服务端配置错误`,
            },
        });
    }
    /**
     * 根据凭证生成去敏用户ID
     * 使用服务端配置的 stats.user_hash_salt 对凭证做 HMAC-SHA256 去敏，生成稳定用户标识。
     * @returns UserIdResponse 生成成功
     * @throws ApiError
     */
    public static postUserId({
        requestBody,
    }: {
        requestBody: UnifiedSaveRequest,
    }): CancelablePromise<UserIdResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/auth/user-id',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                422: `凭证缺失或无效`,
                500: `服务端未配置 user_hash_salt`,
            },
        });
    }
}

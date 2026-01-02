/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { QrCodeCreateResponse } from '../models/QrCodeCreateResponse';
import type { QrCodeStatusResponse } from '../models/QrCodeStatusResponse';
import type { UnifiedSaveRequest } from '../models/UnifiedSaveRequest';
import type { UserIdResponse } from '../models/UserIdResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class AuthService {
    /**
     * 生成登录二维码
     * 为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。客户端需保存返回的 qrId 以轮询授权状态。
     * @returns QrCodeCreateResponse 生成二维码成功
     * @throws ApiError
     */
    public static postQrcode({
        taptapVersion,
    }: {
        /**
         * TapTap 版本：cn（大陆版）或 global（国际版）
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
                500: `服务器内部错误`,
                502: `上游网络错误`,
            },
        });
    }
    /**
     * 轮询二维码授权状态
     * 根据 qr_id 查询当前授权进度。若返回 Pending 且包含 retry_after，客户端应按该秒数后再发起轮询。
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
     * 根据凭证生成去敏用户ID
     * 使用服务端配置的 stats.user_hash_salt 对凭证做 HMAC-SHA256 去敏（取前 16 字节，32 位 hex），用于同一用户的稳定标识。注意：salt 变更会导致 user_id 整体变化。
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
                422: `凭证缺失/无效，或无法识别用户`,
                500: `服务端未配置 user_hash_salt`,
            },
        });
    }
}

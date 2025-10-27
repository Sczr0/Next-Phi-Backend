/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { QrCodeCreateResponse } from '../models/QrCodeCreateResponse';
import type { QrCodeStatusResponse } from '../models/QrCodeStatusResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class AuthService {
    /**
     * 生成登录二维码
     * 为设备申请 TapTap 设备码并返回可扫码的 SVG 二维码（base64）与校验 URL。客户端需保存返回的 qr_id 以轮询授权状态。
     * @returns QrCodeCreateResponse 生成二维码成功
     * @throws ApiError
     */
    public static getQrcode(): CancelablePromise<QrCodeCreateResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/auth/qrcode',
            errors: {
                500: `服务器内部错误`,
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
            errors: {
                404: `二维码不存在或已过期`,
                500: `服务器内部错误`,
            },
        });
    }
}

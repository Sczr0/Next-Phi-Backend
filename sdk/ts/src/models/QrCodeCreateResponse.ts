/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type QrCodeCreateResponse = {
    /**
     * 二维码标识，用于轮询状态
     */
    qr_id: string;
    /**
     * SVG 二维码的 data URL（base64 编码）
     */
    qrcode_base64: string;
    /**
     * 用户在浏览器中访问以确认授权的 URL
     */
    verification_url: string;
};


/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type QrCodeStatusResponse = {
    /**
     * 可选的人类可读提示消息
     */
    message?: string | null;
    /**
     * 若需延后轮询，返回建议的等待秒数
     */
    retryAfter?: number | null;
    /**
     * 若 Confirmed，返回 LeanCloud Session Token
     */
    sessionToken?: string | null;
    /**
     * 当前状态：Pending/Scanned/Confirmed/Error/Expired
     */
    status: string;
};


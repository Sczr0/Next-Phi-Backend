/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type ExternalApiCredentials = {
    /**
     * 外部 API 的访问令牌（如需）
     */
    apiToken?: string | null;
    /**
     * 外部 API 的用户 ID（直连方式之一）
     */
    apiUserId?: string | null;
    /**
     * 外部平台标识，如 "TapTap"/"Bilibili"（与 platformId 配对）
     */
    platform?: string | null;
    /**
     * 外部平台用户唯一标识（与 platform 配对）
     */
    platformId?: string | null;
    /**
     * 外部平台会话令牌（某些平台以此直连）
     */
    sessiontoken?: string | null;
};


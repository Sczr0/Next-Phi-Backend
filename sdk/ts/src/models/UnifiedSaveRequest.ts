/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ExternalApiCredentials } from './ExternalApiCredentials';
/**
 * 统一的存档请求结构
 */
export type UnifiedSaveRequest = {
    externalCredentials?: (null | ExternalApiCredentials);
    /**
     * 官方 LeanCloud 会话令牌
     */
    sessionToken?: string | null;
    /**
     * TapTap 版本选择：cn（大陆版，默认）或 global（国际版）
     */
    taptapVersion?: string | null;
};


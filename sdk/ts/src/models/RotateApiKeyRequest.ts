/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type RotateApiKeyRequest = {
    /**
     * 可选：环境（live/test）
     */
    environment?: string | null;
    /**
     * 可选：旧 key 过渡窗口（秒）
     */
    gracePeriodSecs?: number | null;
    /**
     * 可选：新 key 名称
     */
    name?: string | null;
    /**
     * 可选：新 scopes
     */
    scopes?: any[] | null;
};


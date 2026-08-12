/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type CreateApiKeyRequest = {
    /**
     * 环境：live 或 test（默认 live）
     */
    environment?: string | null;
    /**
     * 过期时间戳（秒，可选）
     */
    expiresAt?: number | null;
    /**
     * Key 名称（控制台展示）
     */
    name: string;
    /**
     * scope 列表（为空则使用默认）
     */
    scopes?: any[] | null;
};


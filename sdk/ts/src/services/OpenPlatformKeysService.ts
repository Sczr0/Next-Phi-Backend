/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApiKeyEventsResponse } from '../models/ApiKeyEventsResponse';
import type { ApiKeyIssueResponse } from '../models/ApiKeyIssueResponse';
import type { ApiKeyListResponse } from '../models/ApiKeyListResponse';
import type { ApiKeyRateLimitResponse } from '../models/ApiKeyRateLimitResponse';
import type { CreateApiKeyRequest } from '../models/CreateApiKeyRequest';
import type { DeleteApiKeyRequest } from '../models/DeleteApiKeyRequest';
import type { OkResponse } from '../models/OkResponse';
import type { RevokeApiKeyRequest } from '../models/RevokeApiKeyRequest';
import type { RotateApiKeyRequest } from '../models/RotateApiKeyRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class OpenPlatformKeysService {
    /**
     * 列出当前开发者 API Keys（掩码）
     * @returns ApiKeyListResponse 查询成功
     * @throws ApiError
     */
    public static getApiKeys({
        includeInactive,
    }: {
        /**
         * 是否包含非 active 的历史 Key（默认 false）
         */
        includeInactive?: boolean,
    }): CancelablePromise<ApiKeyListResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/developer/api-keys',
            query: {
                'includeInactive': includeInactive,
            },
            errors: {
                401: `开发者会话无效`,
                422: `查询参数无效（如 includeInactive 非布尔值）`,
                500: `服务端内部错误（存储未初始化）`,
            },
        });
    }
    /**
     * 创建 API Key（明文仅返回一次）
     * @returns ApiKeyIssueResponse 创建成功
     * @throws ApiError
     */
    public static postCreateApiKey({
        requestBody,
    }: {
        requestBody: CreateApiKeyRequest,
    }): CancelablePromise<ApiKeyIssueResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/developer/api-keys',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `开发者会话无效`,
                422: `参数校验失败`,
                500: `服务端内部错误（如 hash 密钥未配置）`,
            },
        });
    }
    /**
     * 删除 API Key（软删除）
     * @returns OkResponse 删除成功
     * @throws ApiError
     */
    public static postDeleteApiKey({
        keyId,
        requestBody,
    }: {
        /**
         * 待删除的 key_id
         */
        keyId: string,
        requestBody: DeleteApiKeyRequest,
    }): CancelablePromise<OkResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/developer/api-keys/{key_id}/delete',
            path: {
                'key_id': keyId,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `开发者会话无效或无权操作该 Key`,
                404: `API Key 不存在`,
                422: `请求体 JSON 无效`,
                500: `服务端内部错误（存储未初始化）`,
            },
        });
    }
    /**
     * 查询 API Key 事件
     * @returns ApiKeyEventsResponse 查询成功
     * @throws ApiError
     */
    public static getApiKeyEvents({
        keyId,
        limit,
    }: {
        /**
         * key_id
         */
        keyId: string,
        /**
         * 返回条数，默认 100，最大 500
         */
        limit?: number,
    }): CancelablePromise<ApiKeyEventsResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/developer/api-keys/{key_id}/events',
            path: {
                'key_id': keyId,
            },
            query: {
                'limit': limit,
            },
            errors: {
                401: `开发者会话无效或无权操作该 Key`,
                404: `API Key 不存在`,
                422: `查询参数无效（如 limit 非整数）`,
                500: `服务端内部错误（存储未初始化）`,
            },
        });
    }
    /**
     * 查询 API Key 限流窗口信息
     * @returns ApiKeyRateLimitResponse 查询成功
     * @throws ApiError
     */
    public static getApiKeyRateLimit({
        keyId,
        includeClientIp,
        limit,
    }: {
        /**
         * key_id
         */
        keyId: string,
        /**
         * 是否按 client_ip 展开，默认 false
         */
        includeClientIp?: boolean,
        /**
         * 最多返回桶数量，默认 100，最大 500
         */
        limit?: number,
    }): CancelablePromise<ApiKeyRateLimitResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/developer/api-keys/{key_id}/rate-limit',
            path: {
                'key_id': keyId,
            },
            query: {
                'includeClientIp': includeClientIp,
                'limit': limit,
            },
            errors: {
                401: `开发者会话无效或无权操作该 Key`,
                404: `API Key 不存在`,
                422: `查询参数无效（如 includeClientIp 非布尔或 limit 非整数）`,
                500: `服务端内部错误（存储未初始化）`,
            },
        });
    }
    /**
     * 撤销 API Key
     * @returns OkResponse 撤销成功
     * @throws ApiError
     */
    public static postRevokeApiKey({
        keyId,
        requestBody,
    }: {
        /**
         * 待撤销的 key_id
         */
        keyId: string,
        requestBody: RevokeApiKeyRequest,
    }): CancelablePromise<OkResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/developer/api-keys/{key_id}/revoke',
            path: {
                'key_id': keyId,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `开发者会话无效或无权操作该 Key`,
                404: `API Key 不存在`,
                422: `请求体 JSON 无效`,
                500: `服务端内部错误（存储未初始化）`,
            },
        });
    }
    /**
     * 轮换 API Key
     * @returns ApiKeyIssueResponse 轮换成功
     * @throws ApiError
     */
    public static postRotateApiKey({
        keyId,
        requestBody,
    }: {
        /**
         * 待轮换的 key_id
         */
        keyId: string,
        requestBody: RotateApiKeyRequest,
    }): CancelablePromise<ApiKeyIssueResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/developer/api-keys/{key_id}/rotate',
            path: {
                'key_id': keyId,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `开发者会话无效或无权操作该 Key`,
                404: `API Key 不存在`,
                422: `参数校验失败`,
                500: `服务端内部错误（如 hash 密钥未配置）`,
            },
        });
    }
}

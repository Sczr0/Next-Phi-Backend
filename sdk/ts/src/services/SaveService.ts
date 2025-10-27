/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SaveAndRksResponseDoc } from '../models/SaveAndRksResponseDoc';
import type { UnifiedSaveRequest } from '../models/UnifiedSaveRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class SaveService {
    /**
     * 获取并解析玩家存档
     * 支持两种认证方式（官方 sessionToken / 外部凭证）。默认仅返回解析后的存档；当 `calculate_rks=true` 时同时返回玩家 RKS 概览。
     * @returns SaveAndRksResponseDoc 成功解析存档并计算RKS
     * @throws ApiError
     */
    public static getSaveData({
        requestBody,
        calculateRks,
    }: {
        requestBody: UnifiedSaveRequest,
        /**
         * 是否计算玩家RKS（true=计算，默认不计算）
         */
        calculateRks?: boolean,
    }): CancelablePromise<SaveAndRksResponseDoc> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/save',
            query: {
                'calculate_rks': calculateRks,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `请求参数错误`,
                500: `服务器内部错误`,
            },
        });
    }
}

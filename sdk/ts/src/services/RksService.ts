/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { RksHistoryRequest } from '../models/RksHistoryRequest';
import type { RksHistoryResponse } from '../models/RksHistoryResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class RksService {
    /**
     * 查询 RKS 历史变化
     * 通过认证信息查询用户的 RKS 历史变化记录，包括每次提交的 RKS 值和变化量。
     * @returns RksHistoryResponse 成功返回 RKS 历史
     * @throws ApiError
     */
    public static postRksHistory({
        requestBody,
    }: {
        requestBody: RksHistoryRequest,
    }): CancelablePromise<RksHistoryResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/rks/history',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `认证失败/无法识别用户`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
}

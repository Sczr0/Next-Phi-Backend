/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { UnifiedSaveRequest } from './UnifiedSaveRequest';
/**
 * RKS 历史查询请求
 */
export type RksHistoryRequest = {
    /**
     * 认证信息
     */
    auth: UnifiedSaveRequest;
    /**
     * 返回数量（默认 50，最大 200）
     */
    limit?: number | null;
    /**
     * 分页偏移（默认 0）
     */
    offset?: number | null;
};


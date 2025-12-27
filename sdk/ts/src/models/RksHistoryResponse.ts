/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { RksHistoryItem } from './RksHistoryItem';
/**
 * RKS 历史查询响应
 */
export type RksHistoryResponse = {
    /**
     * 当前 RKS
     */
    currentRks: number;
    /**
     * 历史记录列表（按时间倒序）
     */
    items: Array<RksHistoryItem>;
    /**
     * 历史最高 RKS
     */
    peakRks: number;
    /**
     * 总记录数
     */
    total: number;
};


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
     * 是否还有下一页
     */
    hasMore: boolean;
    /**
     * 历史记录列表（按时间倒序）
     */
    items: Array<RksHistoryItem>;
    /**
     * 下一页游标；为空表示已到末尾
     */
    nextCursor?: string | null;
    /**
     * 历史最高 RKS
     */
    peakRks: number;
    /**
     * 总记录数
     */
    total: number;
};

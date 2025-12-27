/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * 单条 RKS 历史记录
 */
export type RksHistoryItem = {
    /**
     * 记录时间（UTC RFC3339）
     */
    createdAt: string;
    /**
     * RKS 值
     */
    rks: number;
    /**
     * 相比上次的变化量
     */
    rksJump: number;
};


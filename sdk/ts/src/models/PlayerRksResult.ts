/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ChartRankingScore } from './ChartRankingScore';
/**
 * 玩家 RKS 计算结果
 */
export type PlayerRksResult = {
    b30_charts: Array<ChartRankingScore>;
    /**
     * 玩家总 RKS （Best27 + AP3）/ 30
     */
    total_rks: number;
};


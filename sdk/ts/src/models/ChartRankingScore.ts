/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Difficulty } from './Difficulty';
/**
 * 单张谱面的 RKS 结果
 */
export type ChartRankingScore = {
    difficulty: Difficulty;
    /**
     * 谱面 RKS 值
     */
    rks: number;
    /**
     * 歌曲 ID
     */
    songId: string;
};


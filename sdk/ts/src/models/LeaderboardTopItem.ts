/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type LeaderboardTopItem = {
    /**
     * 公开别名（如有）
     */
    alias?: string | null;
    /**
     * （可选）AP Top3 列表（当用户允许展示时）
     */
    ap_top3?: any[] | null;
    /**
     * （可选）BestTop3 列表（当用户允许展示时）
     */
    best_top3?: any[] | null;
    /**
     * 名次（竞争排名）
     */
    rank: number;
    /**
     * 总 RKS
     */
    score: number;
    /**
     * 最近更新时间（UTC RFC3339）
     */
    updated_at: string;
    /**
     * 去敏化用户标识（hash 前缀）
     */
    user: string;
};


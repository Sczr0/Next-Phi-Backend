/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type FeatureUsageSummary = {
    /**
     * 事件计数
     */
    count: number;
    /**
     * 功能名（可能值：bestn、bestn_user、single_query、save、song_search）。
     * - bestn：生成 BestN 汇总图
     * - bestn_user：生成用户自报 BestN 图片
     * - single_query：生成单曲成绩图
     * - save：获取并解析玩家存档
     * - song_search：歌曲检索
     */
    feature: string;
    /**
     * 最近一次发生时间（本地时区 RFC3339）
     */
    last_at?: string | null;
};


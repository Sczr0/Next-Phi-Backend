/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type DailyFeatureUsageRow = {
    /**
     * 使用次数
     */
    count: number;
    /**
     * 日期（按 timezone 输出）YYYY-MM-DD
     */
    date: string;
    /**
     * 功能名（bestn/save 等）
     */
    feature: string;
    /**
     * 当日唯一用户数（基于 user_hash 去敏标识；若事件未记录 user_hash，则不会计入）
     */
    uniqueUsers: number;
};


/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DailyFeatureUsageRow } from './DailyFeatureUsageRow';
export type DailyFeaturesResponse = {
    /**
     * 查询结束日期（YYYY-MM-DD，按 timezone 解释）
     */
    end: string;
    /**
     * 可选功能过滤
     */
    featureFilter?: string | null;
    rows: Array<DailyFeatureUsageRow>;
    /**
     * 查询开始日期（YYYY-MM-DD，按 timezone 解释）
     */
    start: string;
    /**
     * 展示统计的时区（IANA 名称）
     */
    timezone: string;
};


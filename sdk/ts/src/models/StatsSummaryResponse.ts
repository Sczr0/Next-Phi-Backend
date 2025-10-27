/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FeatureUsageSummary } from './FeatureUsageSummary';
import type { UniqueUsersSummary } from './UniqueUsersSummary';
export type StatsSummaryResponse = {
    /**
     * 配置中设置的统计起始时间（如有）
     */
    config_start_at?: string | null;
    /**
     * 各功能使用概览
     */
    features: Array<FeatureUsageSummary>;
    /**
     * 全量事件中的最早时间（本地时区）
     */
    first_event_at?: string | null;
    /**
     * 全量事件中的最晚时间（本地时区）
     */
    last_event_at?: string | null;
    /**
     * 展示使用的时区（IANA 名称）
     */
    timezone: string;
    /**
     * 唯一用户统计
     */
    unique_users: UniqueUsersSummary;
};


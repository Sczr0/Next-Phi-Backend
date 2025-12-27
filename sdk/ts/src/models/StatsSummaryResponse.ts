/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { FeatureUsageSummary } from './FeatureUsageSummary';
import type { LatencySummary } from './LatencySummary';
import type { UniqueUsersSummary } from './UniqueUsersSummary';
export type StatsSummaryResponse = {
    actions?: any[] | null;
    /**
     * 配置中设置的统计起始时间（如有）
     */
    configStartAt?: string | null;
    eventsTotal?: number | null;
    featureFilter?: string | null;
    /**
     * 各功能使用概览
     */
    features: Array<FeatureUsageSummary>;
    /**
     * 全量事件中的最早时间（本地时区）
     */
    firstEventAt?: string | null;
    httpErrors?: number | null;
    httpTotal?: number | null;
    instances?: any[] | null;
    /**
     * 全量事件中的最晚时间（本地时区）
     */
    lastEventAt?: string | null;
    latency?: (null | LatencySummary);
    methods?: any[] | null;
    rangeEndAt?: string | null;
    rangeStartAt?: string | null;
    routes?: any[] | null;
    statusCodes?: any[] | null;
    /**
     * 展示使用的时区（IANA 名称）
     */
    timezone: string;
    uniqueIps?: number | null;
    /**
     * 唯一用户统计
     */
    uniqueUsers: UniqueUsersSummary;
};


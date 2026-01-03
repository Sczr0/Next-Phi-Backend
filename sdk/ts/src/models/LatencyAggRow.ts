/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type LatencyAggRow = {
    /**
     * 平均耗时（毫秒）
     */
    avgMs?: number | null;
    /**
     * bucket 标签：day=YYYY-MM-DD；week=week_start(YYYY-MM-DD)；month=month_start(YYYY-MM-01)
     */
    bucket: string;
    /**
     * 样本数
     */
    count: number;
    /**
     * 事件中的 feature（可为空）
     */
    feature?: string | null;
    /**
     * 最大耗时（毫秒）
     */
    maxMs?: number | null;
    /**
     * 事件中的 method（GET/POST 等）
     */
    method?: string | null;
    /**
     * 最小耗时（毫秒）
     */
    minMs?: number | null;
    /**
     * 事件中的 route（MatchedPath）
     */
    route?: string | null;
};


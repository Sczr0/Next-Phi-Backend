/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type DailyAggRow = {
    /**
     * 调用次数
     */
    count: number;
    /**
     * 日期（本地时区）YYYY-MM-DD
     */
    date: string;
    /**
     * 错误次数（status >= 400）
     */
    err_count: number;
    /**
     * 业务功能名（bestn/single_query/save 等）
     */
    feature?: string | null;
    /**
     * HTTP 方法（GET/POST 等）
     */
    method?: string | null;
    /**
     * 路由模板（例如 /image/bn）
     */
    route?: string | null;
};


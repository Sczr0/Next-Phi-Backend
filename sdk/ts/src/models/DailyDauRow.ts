/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type DailyDauRow = {
    /**
     * 当日活跃 IP 数（distinct client_ip_hash；用于覆盖匿名访问）
     */
    activeIps: number;
    /**
     * 当日活跃用户数（distinct user_hash；仅统计能去敏识别的用户）
     */
    activeUsers: number;
    /**
     * 日期（按 timezone 输出）YYYY-MM-DD
     */
    date: string;
};


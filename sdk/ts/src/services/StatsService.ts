/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ArchiveNowResponse } from '../models/ArchiveNowResponse';
import type { DailyAggRow } from '../models/DailyAggRow';
import type { DailyDauResponse } from '../models/DailyDauResponse';
import type { DailyFeaturesResponse } from '../models/DailyFeaturesResponse';
import type { DailyHttpResponse } from '../models/DailyHttpResponse';
import type { StatsSummaryResponse } from '../models/StatsSummaryResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class StatsService {
    /**
     * 手动触发某日归档
     * 将指定日期（默认昨天）的明细导出为 Parquet 文件，落地到配置的归档目录
     * @returns ArchiveNowResponse 归档已触发
     * @throws ApiError
     */
    public static triggerArchiveNow({
        date,
    }: {
        /**
         * 归档日期 YYYY-MM-DD，默认为昨天
         */
        date?: string,
    }): CancelablePromise<ArchiveNowResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/stats/archive/now',
            query: {
                'date': date,
            },
            errors: {
                422: `参数校验失败（日期格式等）`,
                500: `统计存储未初始化/归档失败`,
            },
        });
    }
    /**
     * 按日聚合的统计数据
     * 在 SQLite 明细上进行区间聚合，返回每天每功能/路由的调用与错误次数汇总
     * @returns DailyAggRow 聚合结果
     * @throws ApiError
     */
    public static getDailyStats({
        start,
        end,
        timezone,
        feature,
        route,
        method,
    }: {
        /**
         * 开始日期 YYYY-MM-DD
         */
        start: string,
        /**
         * 结束日期 YYYY-MM-DD
         */
        end: string,
        /**
         * 可选时区 IANA 名称（覆盖配置）
         */
        timezone?: string,
        /**
         * 可选功能名
         */
        feature?: string,
        /**
         * 可选路由模板（MatchedPath）
         */
        route?: string,
        /**
         * 可选 HTTP 方法（GET/POST 等）
         */
        method?: string,
    }): CancelablePromise<Array<DailyAggRow>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/stats/daily',
            query: {
                'start': start,
                'end': end,
                'timezone': timezone,
                'feature': feature,
                'route': route,
                'method': method,
            },
            errors: {
                422: `参数校验失败（日期格式等）`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
    /**
     * 按天输出 DAU（活跃用户数）
     * 按天聚合统计活跃用户数：active_users 基于去敏 user_hash；active_ips 基于去敏 client_ip_hash（HTTP 请求采集）。
     * @returns DailyDauResponse 按天 DAU
     * @throws ApiError
     */
    public static getDailyDau({
        start,
        end,
        timezone,
    }: {
        /**
         * 开始日期 YYYY-MM-DD（按 timezone 解释）
         */
        start: string,
        /**
         * 结束日期 YYYY-MM-DD（按 timezone 解释）
         */
        end: string,
        /**
         * 可选时区 IANA 名称（覆盖配置）
         */
        timezone?: string,
    }): CancelablePromise<DailyDauResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/stats/daily/dau',
            query: {
                'start': start,
                'end': end,
                'timezone': timezone,
            },
            errors: {
                422: `参数校验失败（日期格式/timezone 等）`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
    /**
     * 按天输出功能使用次数
     * 基于 stats 事件明细（feature/action）按天聚合，输出每天各功能的调用次数与当日唯一用户数（user_hash）。
     * @returns DailyFeaturesResponse 按天功能使用次数
     * @throws ApiError
     */
    public static getDailyFeatures({
        start,
        end,
        timezone,
        feature,
    }: {
        /**
         * 开始日期 YYYY-MM-DD（按 timezone 解释）
         */
        start: string,
        /**
         * 结束日期 YYYY-MM-DD（按 timezone 解释）
         */
        end: string,
        /**
         * 可选时区 IANA 名称（覆盖配置）
         */
        timezone?: string,
        /**
         * 可选功能名过滤（bestn/save 等）
         */
        feature?: string,
    }): CancelablePromise<DailyFeaturesResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/stats/daily/features',
            query: {
                'start': start,
                'end': end,
                'timezone': timezone,
                'feature': feature,
            },
            errors: {
                422: `参数校验失败（日期格式/timezone 等）`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
    /**
     * 按天输出 HTTP 错误率（含总错误率）
     * 按天聚合所有 HTTP 请求（route+method）并计算错误率：overall(>=400)、4xx、5xx；同时给出每天总错误率与路由明细。
     * @returns DailyHttpResponse 按天 HTTP 错误率
     * @throws ApiError
     */
    public static getDailyHttp({
        start,
        end,
        timezone,
        route,
        method,
        top,
    }: {
        /**
         * 开始日期 YYYY-MM-DD（按 timezone 解释）
         */
        start: string,
        /**
         * 结束日期 YYYY-MM-DD（按 timezone 解释）
         */
        end: string,
        /**
         * 可选时区 IANA 名称（覆盖配置）
         */
        timezone?: string,
        /**
         * 可选路由模板过滤（MatchedPath）
         */
        route?: string,
        /**
         * 可选 HTTP 方法过滤（GET/POST 等）
         */
        method?: string,
        /**
         * 每天最多返回的路由明细条数（默认 200，最多 200）
         */
        top?: number,
    }): CancelablePromise<DailyHttpResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/stats/daily/http',
            query: {
                'start': start,
                'end': end,
                'timezone': timezone,
                'route': route,
                'method': method,
                'top': top,
            },
            errors: {
                422: `参数校验失败（日期格式/timezone/top 等）`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
    /**
     * 统计总览（唯一用户与功能使用）
     * 提供统计模块关键指标：全局首末事件时间、按功能的使用次数与最近时间、唯一用户总量及来源分布。
     *
     * 功能次数统计中的功能名可能值：
     * - bestn：生成 BestN 汇总图
     * - bestn_user：生成用户自报 BestN 图片
     * - single_query：生成单曲成绩图
     * - save：获取并解析玩家存档
     * - song_search：歌曲检索
     * @returns StatsSummaryResponse 汇总信息
     * @throws ApiError
     */
    public static getStatsSummary({
        start,
        end,
        timezone,
        feature,
        include,
        top,
    }: {
        /**
         * 可选开始日期 YYYY-MM-DD（按 timezone 解释）
         */
        start?: string,
        /**
         * 可选结束日期 YYYY-MM-DD（按 timezone 解释）
         */
        end?: string,
        /**
         * 可选时区 IANA 名称（覆盖配置）
         */
        timezone?: string,
        /**
         * 可选功能名过滤（仅业务维度）
         */
        feature?: string,
        /**
         * 可选额外维度：routes,status,methods,instances,actions,latency,unique_ips,all
         */
        include?: string,
        /**
         * TopN（默认 20，最大 200）
         */
        top?: number,
    }): CancelablePromise<StatsSummaryResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/stats/summary',
            query: {
                'start': start,
                'end': end,
                'timezone': timezone,
                'feature': feature,
                'include': include,
                'top': top,
            },
            errors: {
                422: `参数校验失败（日期格式/timezone/top 等）`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
}

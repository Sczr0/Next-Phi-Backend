/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ArchiveNowResponse } from '../models/ArchiveNowResponse';
import type { DailyAggRow } from '../models/DailyAggRow';
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

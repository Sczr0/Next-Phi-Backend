/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DailyAggRow } from '../models/DailyAggRow';
import type { StatsSummaryResponse } from '../models/StatsSummaryResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class StatsService {
    /**
     * 按日聚合的统计数据
     * 在 SQLite 明细上进行区间聚合，返回每天每功能/路由的调用与错误次数汇总
     * @returns DailyAggRow
     * @throws ApiError
     */
    public static getDailyStats({
        start,
        end,
        feature,
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
    }): CancelablePromise<Array<DailyAggRow>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/stats/daily',
            query: {
                'start': start,
                'end': end,
                'feature': feature,
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
     * @returns StatsSummaryResponse
     * @throws ApiError
     */
    public static getStatsSummary(): CancelablePromise<StatsSummaryResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/stats/summary',
        });
    }
}

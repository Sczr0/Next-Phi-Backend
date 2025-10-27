/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { AliasRequest } from '../models/AliasRequest';
import type { LeaderboardTopResponse } from '../models/LeaderboardTopResponse';
import type { MeResponse } from '../models/MeResponse';
import type { ProfileUpdateRequest } from '../models/ProfileUpdateRequest';
import type { PublicProfileResponse } from '../models/PublicProfileResponse';
import type { UnifiedSaveRequest } from '../models/UnifiedSaveRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class LeaderboardService {
    /**
     * 设置/更新公开别名（幂等）
     * @returns any ok
     * @throws ApiError
     */
    public static putAlias({
        requestBody,
    }: {
        requestBody: AliasRequest,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/leaderboard/alias',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                409: `别名被占用`,
                422: `别名非法`,
            },
        });
    }
    /**
     * 更新公开资料开关（文字展示）
     * @returns any ok
     * @throws ApiError
     */
    public static putProfile({
        requestBody,
    }: {
        requestBody: ProfileUpdateRequest,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/leaderboard/profile',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * 按排名区间获取玩家（按RKS）
     * 可传入单个 rank，或 [start,end] / [start,count] 区间获取玩家信息。采用与 TOP 相同的稳定排序与公开过滤。
     * @returns LeaderboardTopResponse
     * @throws ApiError
     */
    public static getByRank({
        rank,
        start,
        end,
        count,
    }: {
        /**
         * 单个排名（1-based）
         */
        rank?: number,
        /**
         * 起始排名（1-based）
         */
        start?: number,
        /**
         * 结束排名（包含）
         */
        end?: number,
        /**
         * 返回数量（与 start 组合使用）
         */
        count?: number,
    }): CancelablePromise<LeaderboardTopResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/leaderboard/rks/by-rank',
            query: {
                'rank': rank,
                'start': start,
                'end': end,
                'count': count,
            },
        });
    }
    /**
     * 我的名次（按RKS）
     * 通过认证信息推导用户身份，返回名次、分数、总量与百分位（竞争排名）
     * @returns MeResponse
     * @throws ApiError
     */
    public static postMe({
        requestBody,
    }: {
        requestBody: UnifiedSaveRequest,
    }): CancelablePromise<MeResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/leaderboard/rks/me',
            body: requestBody,
            mediaType: 'application/json',
        });
    }
    /**
     * 排行榜TOP（按RKS）
     * 返回公开玩家的RKS排行榜。若玩家开启展示，将在条目中附带BestTop3/APTop3文字数据。
     * @returns LeaderboardTopResponse
     * @throws ApiError
     */
    public static getTop({
        limit,
        offset,
    }: {
        /**
         * 每页数量，默认50，最大200
         */
        limit?: number,
        /**
         * 偏移量
         */
        offset?: number,
    }): CancelablePromise<LeaderboardTopResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/leaderboard/rks/top',
            query: {
                'limit': limit,
                'offset': offset,
            },
        });
    }
    /**
     * 公开玩家资料（纯文字）
     * @returns PublicProfileResponse
     * @throws ApiError
     */
    public static getPublicProfile({
        alias,
    }: {
        /**
         * 公开别名
         */
        alias: string,
    }): CancelablePromise<PublicProfileResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/public/profile/{alias}',
            path: {
                'alias': alias,
            },
            errors: {
                404: `not found`,
            },
        });
    }
}

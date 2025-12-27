/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { AliasRequest } from '../models/AliasRequest';
import type { ForceAliasRequest } from '../models/ForceAliasRequest';
import type { LeaderboardTopResponse } from '../models/LeaderboardTopResponse';
import type { MeResponse } from '../models/MeResponse';
import type { OkAliasResponse } from '../models/OkAliasResponse';
import type { OkResponse } from '../models/OkResponse';
import type { ProfileUpdateRequest } from '../models/ProfileUpdateRequest';
import type { PublicProfileResponse } from '../models/PublicProfileResponse';
import type { ResolveRequest } from '../models/ResolveRequest';
import type { SuspiciousItem } from '../models/SuspiciousItem';
import type { UnifiedSaveRequest } from '../models/UnifiedSaveRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class LeaderboardService {
    /**
     * 管理员强制设置/回收别名（会从原持有人移除）
     * 需要在 Header 中提供 X-Admin-Token，令牌来源于 config.leaderboard.admin_tokens。
     * @returns OkAliasResponse 设置成功
     * @throws ApiError
     */
    public static postAliasForce({
        xAdminToken,
        requestBody,
    }: {
        /**
         * 管理员令牌（config.leaderboard.admin_tokens）
         */
        xAdminToken: string,
        requestBody: ForceAliasRequest,
    }): CancelablePromise<OkAliasResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/admin/leaderboard/alias/force',
            headers: {
                'X-Admin-Token': xAdminToken,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `管理员令牌缺失/无效`,
                422: `参数校验失败（别名非法等）`,
                500: `统计存储未初始化/写入失败`,
            },
        });
    }
    /**
     * 审核可疑用户（approved/shadow/banned/rejected）
     * 需要在 Header 中提供 X-Admin-Token，令牌来源于 config.leaderboard.admin_tokens。
     * @returns OkResponse 处理成功
     * @throws ApiError
     */
    public static postResolve({
        xAdminToken,
        requestBody,
    }: {
        /**
         * 管理员令牌（config.leaderboard.admin_tokens）
         */
        xAdminToken: string,
        requestBody: ResolveRequest,
    }): CancelablePromise<OkResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/admin/leaderboard/resolve',
            headers: {
                'X-Admin-Token': xAdminToken,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `管理员令牌缺失/无效`,
                422: `参数校验失败（status 非法等）`,
                500: `统计存储未初始化/写入失败`,
            },
        });
    }
    /**
     * 可疑用户列表
     * 需要在 Header 中提供 X-Admin-Token，令牌来源于 config.leaderboard.admin_tokens。
     * @returns SuspiciousItem 可疑列表
     * @throws ApiError
     */
    public static getSuspicious({
        xAdminToken,
        minScore,
        limit,
    }: {
        /**
         * 管理员令牌（config.leaderboard.admin_tokens）
         */
        xAdminToken: string,
        /**
         * 最小可疑分，默认0.6
         */
        minScore?: number,
        /**
         * 返回数量，默认 100
         */
        limit?: number,
    }): CancelablePromise<Array<SuspiciousItem>> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/admin/leaderboard/suspicious',
            headers: {
                'X-Admin-Token': xAdminToken,
            },
            query: {
                'min_score': minScore,
                'limit': limit,
            },
            errors: {
                401: `管理员令牌缺失/无效`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
    /**
     * 设置/更新公开别名（幂等）
     * @returns OkAliasResponse 设置成功
     * @throws ApiError
     */
    public static putAlias({
        requestBody,
    }: {
        requestBody: AliasRequest,
    }): CancelablePromise<OkAliasResponse> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/leaderboard/alias',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                409: `别名被占用`,
                422: `别名非法`,
                500: `统计存储未初始化/写入失败/无法识别用户`,
            },
        });
    }
    /**
     * 更新公开资料开关（文字展示）
     * @returns OkResponse 更新成功
     * @throws ApiError
     */
    public static putProfile({
        requestBody,
    }: {
        requestBody: ProfileUpdateRequest,
    }): CancelablePromise<OkResponse> {
        return __request(OpenAPI, {
            method: 'PUT',
            url: '/leaderboard/profile',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                422: `参数校验失败（例如配置禁止公开）`,
                500: `统计存储未初始化/更新失败/无法识别用户`,
            },
        });
    }
    /**
     * 按排名区间获取玩家（按RKS）
     * 可传入单个 rank，或 [start,end] / [start,count] 区间获取玩家信息。采用与 TOP 相同的稳定排序与公开过滤。
     * @returns LeaderboardTopResponse 区间结果
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
            errors: {
                422: `参数校验失败（缺少 rank/start 等）`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
    /**
     * 我的名次（按RKS）
     * 通过认证信息推导用户身份，返回名次、分数、总量与百分位（竞争排名）
     * @returns MeResponse 查询成功
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
            errors: {
                500: `统计存储未初始化/查询失败/无法识别用户`,
            },
        });
    }
    /**
     * 排行榜TOP（按RKS）
     * 返回公开玩家的RKS排行榜。若玩家开启展示，将在条目中附带BestTop3/APTop3文字数据。
     * @returns LeaderboardTopResponse 排行榜 TOP
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
            errors: {
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
    /**
     * 公开玩家资料（纯文字）
     * @returns PublicProfileResponse 公开资料
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
                404: `未找到（别名不存在或未公开）`,
                500: `统计存储未初始化/查询失败`,
            },
        });
    }
}

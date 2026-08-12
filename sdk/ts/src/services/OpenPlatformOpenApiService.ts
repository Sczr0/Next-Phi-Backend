/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LeaderboardTopResponse } from '../models/LeaderboardTopResponse';
import type { QrCodeCreateResponse } from '../models/QrCodeCreateResponse';
import type { QrCodeStatusResponse } from '../models/QrCodeStatusResponse';
import type { RenderBnRequest } from '../models/RenderBnRequest';
import type { RenderSongRequest } from '../models/RenderSongRequest';
import type { RksHistoryRequest } from '../models/RksHistoryRequest';
import type { RksHistoryResponse } from '../models/RksHistoryResponse';
import type { SaveApiResponse } from '../models/SaveApiResponse';
import type { SongSearchResult } from '../models/SongSearchResult';
import type { UnifiedSaveRequest } from '../models/UnifiedSaveRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class OpenPlatformOpenApiService {
    /**
     * Open API: 生成 TapTap 登录二维码
     * 开放平台二维码登录入口。需要 X-OpenApi-Token，且 API Key 包含 profile.read scope。
     * @returns QrCodeCreateResponse 生成二维码成功
     * @throws ApiError
     */
    public static openAuthQrcode({
        taptapVersion,
    }: {
        /**
         * TapTap 版本：cn（大陆版）或 global（国际版）
         */
        taptapVersion?: string,
    }): CancelablePromise<QrCodeCreateResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/open/auth/qrcode',
            query: {
                'taptapVersion': taptapVersion,
            },
            errors: {
                401: `Token 缺失、无效、被吊销或已过期，或 TapTap 上游认证失败`,
                403: `Scope 不足或请求触发限流`,
                422: `参数校验失败（taptapVersion 非法）`,
                500: `服务器内部错误`,
                502: `上游网络错误（TapTap 调用失败）`,
                504: `上游请求超时`,
            },
        });
    }
    /**
     * Open API: 轮询 TapTap 二维码登录状态
     * 开放平台二维码登录状态轮询入口。需要 X-OpenApi-Token，且 API Key 包含 profile.read scope。
     * @returns QrCodeStatusResponse 状态返回
     * @throws ApiError
     */
    public static openAuthQrcodeStatus({
        qrId,
    }: {
        /**
         * 二维码 ID
         */
        qrId: string,
    }): CancelablePromise<QrCodeStatusResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/open/auth/qrcode/{qr_id}/status',
            path: {
                'qr_id': qrId,
            },
            errors: {
                401: `Token 缺失、无效、被吊销或已过期`,
                403: `Scope 不足或请求触发限流`,
            },
        });
    }
    /**
     * Open API: Render BestN Image (SVG Only)
     * Open platform endpoint for BestN image rendering. Requires X-OpenApi-Token and scope profile.read. Only format=svg is allowed.
     * @returns string Request succeeded.
     * @throws ApiError
     */
    public static openImageBn({
        requestBody,
        format,
    }: {
        requestBody: RenderBnRequest,
        /**
         * Only supports svg. Omit or pass svg.
         */
        format?: string,
    }): CancelablePromise<string> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/open/image/bn',
            query: {
                'format': format,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Bad request (missing credentials).`,
                401: `Token is missing, invalid, revoked or expired.`,
                403: `Scope is insufficient or request is rate limited.`,
                422: `Validation failed (only format=svg is allowed).`,
                500: `Internal server error.`,
            },
        });
    }
    /**
     * Open API: Render Song Image (SVG Only)
     * Open platform endpoint for song image rendering. Requires X-OpenApi-Token and scope profile.read. Only format=svg is allowed.
     * @returns string Request succeeded.
     * @throws ApiError
     */
    public static openImageSong({
        requestBody,
        format,
    }: {
        requestBody: RenderSongRequest,
        /**
         * Only supports svg. Omit or pass svg.
         */
        format?: string,
    }): CancelablePromise<string> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/open/image/song',
            query: {
                'format': format,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Bad request (missing credentials).`,
                401: `Token is missing, invalid, revoked or expired.`,
                403: `Scope is insufficient or request is rate limited.`,
                404: `Song not found (unique search).`,
                409: `Song result is not unique (unique search).`,
                422: `Validation failed (only format=svg is allowed).`,
                500: `Internal server error.`,
            },
        });
    }
    /**
     * Open API: Leaderboard Range
     * Open platform endpoint for public RKS rank range query. Requires X-OpenApi-Token and scope public.read.
     * @returns LeaderboardTopResponse Request succeeded.
     * @throws ApiError
     */
    public static openGetLeaderboardByRank({
        rank,
        start,
        end,
        count,
        lite,
    }: {
        /**
         * Single rank (1-based).
         */
        rank?: number,
        /**
         * Start rank (1-based).
         */
        start?: number,
        /**
         * End rank (inclusive).
         */
        end?: number,
        /**
         * Item count (combined with start, max 200).
         */
        count?: number,
        /**
         * Lite mode: omit bestTop3/apTop3 (default false).
         */
        lite?: boolean,
    }): CancelablePromise<LeaderboardTopResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/open/leaderboard/rks/by-rank',
            query: {
                'rank': rank,
                'start': start,
                'end': end,
                'count': count,
                'lite': lite,
            },
            errors: {
                401: `Token is missing, invalid, revoked or expired.`,
                403: `Scope is insufficient or request is rate limited.`,
                422: `Validation failed (missing rank/start, etc.).`,
                500: `Stats storage not initialized / query failed.`,
            },
        });
    }
    /**
     * Open API: Leaderboard Top
     * Open platform endpoint for public RKS top list. Requires X-OpenApi-Token and scope public.read.
     * @returns LeaderboardTopResponse Request succeeded.
     * @throws ApiError
     */
    public static openGetLeaderboardTop({
        limit,
        offset,
        cursor,
        afterScore,
        afterUpdated,
        afterUser,
        lite,
    }: {
        /**
         * Items per page, default 50; max 200 normally, max 1000 with lite=true.
         */
        limit?: number,
        /**
         * Offset.
         */
        offset?: number,
        /**
         * Encrypted cursor; takes precedence over offset and after_*.
         */
        cursor?: string,
        /**
         * Legacy cursor: last item score (used with after_updated/after_user).
         */
        afterScore?: number,
        /**
         * Legacy cursor: last item updatedAt (RFC3339).
         */
        afterUpdated?: string,
        /**
         * Legacy cursor: last item masked user (hash prefix).
         */
        afterUser?: string,
        /**
         * Lite mode: omit bestTop3/apTop3 (default false).
         */
        lite?: boolean,
    }): CancelablePromise<LeaderboardTopResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/open/leaderboard/rks/top',
            query: {
                'limit': limit,
                'offset': offset,
                'cursor': cursor,
                'after_score': afterScore,
                'after_updated': afterUpdated,
                'after_user': afterUser,
                'lite': lite,
            },
            errors: {
                401: `Token is missing, invalid, revoked or expired.`,
                403: `Scope is insufficient or request is rate limited.`,
                422: `Invalid cursor or after_* parameters.`,
                500: `Stats storage not initialized / query failed.`,
            },
        });
    }
    /**
     * Open API: RKS History
     * Open platform endpoint for user RKS history. Requires X-OpenApi-Token and scope profile.read.
     * @returns RksHistoryResponse Request succeeded.
     * @throws ApiError
     */
    public static openPostRksHistory({
        requestBody,
    }: {
        requestBody: RksHistoryRequest,
    }): CancelablePromise<RksHistoryResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/open/rks/history',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                401: `Token is missing, invalid, revoked or expired.`,
                403: `Scope is insufficient or request is rate limited.`,
                422: `Validation failed (invalid JSON body / invalid cursor).`,
                500: `Stats storage not initialized / query failed.`,
            },
        });
    }
    /**
     * Open API: Parse Save Data
     * Open platform endpoint for save parsing. Requires X-OpenApi-Token and scope profile.read.
     * @returns SaveApiResponse Request succeeded.
     * @throws ApiError
     */
    public static openSaveData({
        requestBody,
        calculateRks,
    }: {
        requestBody: UnifiedSaveRequest,
        /**
         * Set true to include RKS calculation result.
         */
        calculateRks?: boolean,
    }): CancelablePromise<SaveApiResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/open/save',
            query: {
                'calculate_rks': calculateRks,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Bad request (missing/invalid credentials).`,
                401: `Token is missing, invalid, revoked or expired, or upstream authentication failed.`,
                403: `Scope is insufficient or request is rate limited.`,
                422: `Validation failed / save data invalid.`,
                500: `Internal server error.`,
                502: `Upstream network error.`,
                504: `Upstream timeout.`,
            },
        });
    }
    /**
     * Open API: Search Songs
     * Open platform endpoint for song search. Requires X-OpenApi-Token and scope public.read.
     * @returns SongSearchResult Request succeeded (single SongInfo when unique=true, otherwise a paged result).
     * @throws ApiError
     */
    public static openSearchSongs({
        q,
        unique,
        mode,
        limit,
        offset,
    }: {
        /**
         * Search query string (required).
         */
        q: string,
        /**
         * Whether to force a unique match (optional).
         */
        unique?: boolean,
        /**
         * Multi-keyword mode (optional: and/or).
         */
        mode?: string,
        /**
         * Max items (optional, default 20, max 100, min 1).
         */
        limit?: number,
        /**
         * Result offset (optional, default 0).
         */
        offset?: number,
    }): CancelablePromise<SongSearchResult> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/open/songs/search',
            query: {
                'q': q,
                'unique': unique,
                'mode': mode,
                'limit': limit,
                'offset': offset,
            },
            errors: {
                400: `Bad request (q is empty).`,
                401: `Token is missing, invalid, revoked or expired.`,
                403: `Scope is insufficient or request is rate limited.`,
                404: `Not found (unique=true, no match).`,
                409: `Not unique (unique=true, multiple matches).`,
                422: `Validation failed (missing q / q too long / invalid limit / invalid mode).`,
                500: `Internal server error.`,
            },
        });
    }
}

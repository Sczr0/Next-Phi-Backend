/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LeaderboardTopItem } from './LeaderboardTopItem';
export type LeaderboardTopResponse = {
    items: Array<LeaderboardTopItem>;
    nextAfterScore?: number | null;
    nextAfterUpdated?: string | null;
    /**
     * 旧版下一页游标：去敏化用户标识（与 `items[].user` 同规则）。
     */
    nextAfterUser?: string | null;
    /**
     * 下一页加密游标。客户端应优先使用该字段继续分页。
     */
    nextCursor?: string | null;
    total: number;
};

/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LeaderboardTopItem } from './LeaderboardTopItem';
export type LeaderboardTopResponse = {
    items: Array<LeaderboardTopItem>;
    nextAfterScore?: number | null;
    nextAfterUpdated?: string | null;
    nextAfterUser?: string | null;
    total: number;
};


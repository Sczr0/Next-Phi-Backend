/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LeaderboardTopItem } from './LeaderboardTopItem';
export type LeaderboardTopResponse = {
    items: Array<LeaderboardTopItem>;
    next_after_score?: number | null;
    next_after_updated?: string | null;
    next_after_user?: string | null;
    total: number;
};


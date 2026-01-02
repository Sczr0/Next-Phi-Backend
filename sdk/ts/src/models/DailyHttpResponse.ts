/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DailyHttpRouteRow } from './DailyHttpRouteRow';
import type { DailyHttpTotalRow } from './DailyHttpTotalRow';
export type DailyHttpResponse = {
    end: string;
    methodFilter?: string | null;
    routeFilter?: string | null;
    routes: Array<DailyHttpRouteRow>;
    start: string;
    timezone: string;
    totals: Array<DailyHttpTotalRow>;
};


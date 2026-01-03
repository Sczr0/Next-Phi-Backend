/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { LatencyAggRow } from './LatencyAggRow';
export type LatencyAggResponse = {
    /**
     * day/week/month
     */
    bucket: string;
    end: string;
    featureFilter?: string | null;
    methodFilter?: string | null;
    routeFilter?: string | null;
    rows: Array<LatencyAggRow>;
    start: string;
    timezone: string;
};


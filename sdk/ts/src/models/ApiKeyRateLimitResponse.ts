/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ApiKeyRateLimitBucketItem } from './ApiKeyRateLimitBucketItem';
export type ApiKeyRateLimitResponse = {
    bucketCount: number;
    buckets: Array<ApiKeyRateLimitBucketItem>;
    keyId: string;
    minuteSlot: number;
    perMinuteLimit: number;
    strategy: string;
    totalRequestCount: number;
    windowEndTs: number;
    windowStartTs: number;
};


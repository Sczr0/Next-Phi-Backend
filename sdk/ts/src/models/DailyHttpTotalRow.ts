/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type DailyHttpTotalRow = {
    clientErrorRate: number;
    clientErrors: number;
    date: string;
    /**
     * errors / total（total=0 时为 0）
     */
    errorRate: number;
    errors: number;
    serverErrorRate: number;
    serverErrors: number;
    total: number;
};


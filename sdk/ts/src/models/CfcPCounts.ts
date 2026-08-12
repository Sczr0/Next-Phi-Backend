/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * C/FC/P 成绩数量（累计口径）
 *
 * 说明：按需求定义 C<FC<P，且 FC 的成绩同时计入 C，P 的成绩同时计入 FC 与 C。
 */
export type CfcPCounts = {
    /**
     * Clear 数量（包含 FC 与 P）
     */
    'C': number;
    /**
     * Full Combo 数量（包含 P）
     */
    FC: number;
    /**
     * Perfect 数量
     */
    'P': number;
};


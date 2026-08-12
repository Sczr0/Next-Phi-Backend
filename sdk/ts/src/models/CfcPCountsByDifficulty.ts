/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CfcPCounts } from './CfcPCounts';
/**
 * 按难度统计的 C/FC/P 成绩数量
 *
 * JSON 结构使用大写键名（EZ/HD/IN/AT），保证“各个难度”恒存在（即使为 0）。
 */
export type CfcPCountsByDifficulty = {
    AT: CfcPCounts;
    EZ: CfcPCounts;
    HD: CfcPCounts;
    IN: CfcPCounts;
};


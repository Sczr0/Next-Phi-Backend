/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { CfcPCountsByDifficulty } from './CfcPCountsByDifficulty';
import type { ParsedSaveDoc } from './ParsedSaveDoc';
import type { PlayerRksResult } from './PlayerRksResult';
export type SaveAndRksResponseDoc = {
    /**
     * 按难度统计的 C/FC/P 成绩数量（仅 calculate_rks=true 时返回）
     */
    gradeCounts: CfcPCountsByDifficulty;
    /**
     * 玩家 RKS 概览
     */
    rks: PlayerRksResult;
    /**
     * 解析后的存档对象
     */
    save: ParsedSaveDoc;
};


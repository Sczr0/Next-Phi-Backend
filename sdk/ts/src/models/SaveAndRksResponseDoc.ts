/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ParsedSaveDoc } from './ParsedSaveDoc';
import type { PlayerRksResult } from './PlayerRksResult';
export type SaveAndRksResponseDoc = {
    /**
     * 玩家 RKS 概览
     */
    rks: PlayerRksResult;
    /**
     * 解析后的存档对象
     */
    save: ParsedSaveDoc;
};


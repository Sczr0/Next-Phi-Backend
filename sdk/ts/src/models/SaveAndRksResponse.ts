/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { PlayerRksResult } from './PlayerRksResult';
export type SaveAndRksResponse = {
    /**
     * 计算得到的玩家 RKS 概览
     */
    rks: PlayerRksResult;
    /**
     * 解析后的存档对象（等价于 SaveResponse.data）
     */
    save: any;
};


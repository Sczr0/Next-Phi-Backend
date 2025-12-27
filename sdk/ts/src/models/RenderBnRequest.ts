/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { Theme } from './Theme';
import type { UnifiedSaveRequest } from './UnifiedSaveRequest';
/**
 * BN 渲染请求体
 */
export type RenderBnRequest = (UnifiedSaveRequest & {
    /**
     * 是否将封面等资源内嵌到 PNG（默认为 false）
     */
    embedImages?: boolean;
    /**
     * 取前 N 条 RKS 最高的成绩（默认 30）
     */
    'n'?: number;
    /**
     * 可选：用于显示的玩家昵称（若未提供且无法从服务端获取，将使用默认占位）
     */
    nickname?: string | null;
    /**
     * 渲染主题：white/black（默认 black）
     */
    theme?: Theme;
});


/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ImageFormat } from './ImageFormat';
import type { Theme } from './Theme';
import type { UnifiedSaveRequest } from './UnifiedSaveRequest';
/**
 * BN 渲染请求体
 */
export type RenderBnRequest = (UnifiedSaveRequest & {
    /**
     * 是否将封面等资源内嵌到 PNG（默认为 false）
     */
    embed_images?: boolean;
    /**
     * 输出图片格式（png/jpeg，默认 png）
     */
    format?: ImageFormat;
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
    /**
     * 目标宽度像素（可选；不填使用默认 1200）。用于下采样以减小体积。
     */
    width?: number | null;
});


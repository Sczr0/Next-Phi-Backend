/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { UnifiedSaveRequest } from './UnifiedSaveRequest';
/**
 * 单曲渲染请求体
 */
export type RenderSongRequest = (UnifiedSaveRequest & {
    /**
     * 是否将封面等资源内嵌到 PNG（默认为 false）
     */
    embed_images?: boolean;
    /**
     * 可选：用于显示的玩家昵称
     */
    nickname?: string | null;
    /**
     * 歌曲 ID 或名称
     */
    song: string;
});


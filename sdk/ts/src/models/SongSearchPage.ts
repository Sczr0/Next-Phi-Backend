/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SongInfo } from './SongInfo';
/**
 * 分页响应（用于非 unique 查询）。
 */
export type SongSearchPage = {
    hasMore: boolean;
    items: Array<SongInfo>;
    limit: number;
    nextOffset?: number | null;
    offset: number;
    total: number;
};


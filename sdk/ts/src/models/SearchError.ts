/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SongInfo } from './SongInfo';
/**
 * 搜索错误类型
 */
export type SearchError = ('NotFound' | {
    /**
     * 结果不唯一（返回所有候选项，以便提示歧义）
     */
    NotUnique: {
        candidates: Array<SongInfo>;
    };
});


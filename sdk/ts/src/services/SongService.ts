/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SongSearchResult } from '../models/SongSearchResult';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class SongService {
    /**
     * 歌曲检索（支持别名与模糊匹配）
     * 默认按 ID/官方名/别名进行搜索。显式传 `mode=and|or` 时启用多关键词搜索，支持空格分词、双引号短语与前缀 `-` 排除。`unique=true` 时期望唯一命中，未命中返回 404，多命中返回 409。
     * @returns SongSearchResult 查询成功（unique=true 时返回单个对象，否则为分页对象）
     * @throws ApiError
     */
    public static searchSongs({
        q,
        unique,
        mode,
        limit,
        offset,
    }: {
        /**
         * 查询字符串
         */
        q: string,
        /**
         * 是否强制唯一匹配（可选）
         */
        unique?: boolean,
        /**
         * 多关键词模式（可选：and/or）。仅显式传入时启用多关键词搜索
         */
        mode?: string,
        /**
         * 最大返回条数（可选，默认 20，上限 100，最小 1）
         */
        limit?: number,
        /**
         * 结果偏移（可选，默认 0）
         */
        offset?: number,
    }): CancelablePromise<SongSearchResult> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/songs/search',
            query: {
                'q': q,
                'unique': unique,
                'mode': mode,
                'limit': limit,
                'offset': offset,
            },
            errors: {
                400: `请求参数错误（q 为空字符串）`,
                404: `未找到匹配项`,
                409: `结果不唯一`,
                422: `参数校验错误（缺少 q 参数 / q 过长 / limit 无效 / mode 非法）`,
                500: `服务器内部错误`,
            },
        });
    }
}

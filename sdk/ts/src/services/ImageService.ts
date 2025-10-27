/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { RenderBnRequest } from '../models/RenderBnRequest';
import type { RenderSongRequest } from '../models/RenderSongRequest';
import type { RenderUserBnRequest } from '../models/RenderUserBnRequest';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class ImageService {
    /**
     * 生成 BestN 汇总图片
     * 从官方/外部存档解析玩家成绩，按 RKS 值排序取前 N 条生成 BestN 概览（PNG）。可选内嵌封面与主题切换。
     * @returns any PNG bytes of BN image
     * @throws ApiError
     */
    public static renderBn({
        requestBody,
        format,
        width,
    }: {
        requestBody: RenderBnRequest,
        /**
         * 输出格式：png|jpeg，默认 png
         */
        format?: string,
        /**
         * 目标宽度像素：按宽度同比例缩放
         */
        width?: number,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/image/bn',
            query: {
                'format': format,
                'width': width,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Bad request`,
                500: `Renderer error`,
            },
        });
    }
    /**
     * 生成用户自报成绩的 BestN 图片
     * 无需存档，直接提交若干条用户自报成绩，计算 RKS 排序并生成 BestN 图片；支持水印解除口令。
     * @returns any PNG bytes of user BN image
     * @throws ApiError
     */
    public static renderBnUser({
        requestBody,
        format,
        width,
    }: {
        requestBody: RenderUserBnRequest,
        /**
         * 输出格式：png|jpeg，默认 png
         */
        format?: string,
        /**
         * 目标宽度像素：按宽度同比例缩放
         */
        width?: number,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/image/bn/user',
            query: {
                'format': format,
                'width': width,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Bad request`,
                500: `Renderer error`,
            },
        });
    }
    /**
     * 生成单曲成绩图片
     * 从存档中定位指定歌曲（支持 ID/名称），展示四难度成绩、RKS、推分建议等信息（PNG）。
     * @returns any PNG bytes of song image
     * @throws ApiError
     */
    public static renderSong({
        requestBody,
        format,
        width,
    }: {
        requestBody: RenderSongRequest,
        /**
         * 输出格式：png|jpeg，默认 png
         */
        format?: string,
        /**
         * 目标宽度像素：按宽度同比例缩放
         */
        width?: number,
    }): CancelablePromise<any> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/image/song',
            query: {
                'format': format,
                'width': width,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `Bad request`,
                500: `Renderer error`,
            },
        });
    }
}

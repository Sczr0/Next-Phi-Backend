/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { BinaryImage } from '../models/BinaryImage';
import type { PublicKeyResponse } from '../models/PublicKeyResponse';
import type { RenderBnRequest } from '../models/RenderBnRequest';
import type { RenderSongRequest } from '../models/RenderSongRequest';
import type { RenderUserBnRequest } from '../models/RenderUserBnRequest';
import type { VerifyRequest } from '../models/VerifyRequest';
import type { VerifyResponse } from '../models/VerifyResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class ImageService {
    /**
     * 生成 BestN 汇总图片
     * 从官方/外部存档解析玩家成绩（或使用 Authorization: Bearer 内嵌凭证），按 RKS 值排序取前 N 条生成 BestN 概览（PNG）。可选内嵌封面与主题切换。
     * @returns BinaryImage 图片（由 query format 决定）
     * @throws ApiError
     */
    public static renderBn({
        requestBody,
        format,
        template,
        width,
        webpQuality,
        webpLossless,
    }: {
        requestBody: RenderBnRequest,
        /**
         * 输出格式：png|jpeg|webp|svg（jpeg 接受别名 jpg），默认 png；未知值回落 png
         */
        format?: string,
        /**
         * SVG 模板 ID：对应 resources/templates/image/bn/{id}.svg.jinja（不传则使用内置手写 SVG）
         */
        template?: string,
        /**
         * 目标宽度像素：按宽度同比例缩放
         */
        width?: number,
        /**
         * WebP 质量：1-100（仅在 format=webp 时有效，默认 80）
         */
        webpQuality?: number,
        /**
         * WebP 无损模式（仅在 format=webp 时有效，默认 false）
         */
        webpLossless?: boolean,
    }): CancelablePromise<BinaryImage> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/image/bn',
            query: {
                'format': format,
                'template': template,
                'width': width,
                'webp_quality': webpQuality,
                'webp_lossless': webpLossless,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `请求参数错误/认证缺失`,
                401: `Bearer 令牌无效或身份推导失败`,
                403: `用户已被封禁`,
                422: `参数校验失败/渲染错误`,
                500: `服务器内部错误`,
            },
        });
    }
    /**
     * 生成用户自报成绩的 BestN 图片
     * 无需存档，直接提交若干条用户自报成绩，计算 RKS 排序并生成 BestN 图片；支持水印解除口令。
     * @returns BinaryImage 图片（由 query format 决定）
     * @throws ApiError
     */
    public static renderBnUser({
        requestBody,
        format,
        template,
        width,
        webpQuality,
        webpLossless,
    }: {
        requestBody: RenderUserBnRequest,
        /**
         * 输出格式：png|jpeg|webp|svg（jpeg 接受别名 jpg），默认 png；未知值回落 png
         */
        format?: string,
        /**
         * SVG 模板 ID：对应 resources/templates/image/bn/{id}.svg.jinja（不传则使用内置手写 SVG）
         */
        template?: string,
        /**
         * 目标宽度像素：按宽度同比例缩放
         */
        width?: number,
        /**
         * WebP 质量：1-100（仅在 format=webp 时有效，默认 80）
         */
        webpQuality?: number,
        /**
         * WebP 无损模式（仅在 format=webp 时有效，默认 false）
         */
        webpLossless?: boolean,
    }): CancelablePromise<BinaryImage> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/image/bn/user',
            query: {
                'format': format,
                'template': template,
                'width': width,
                'webp_quality': webpQuality,
                'webp_lossless': webpLossless,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                404: `歌曲未找到（unique search）`,
                409: `歌曲结果不唯一（unique search）`,
                422: `参数校验失败/请求体 JSON 无效/渲染错误`,
                500: `服务器内部错误`,
            },
        });
    }
    /**
     * 生成单曲成绩图片
     * 从存档中定位指定歌曲（支持 ID/名称，或使用 Authorization: Bearer 内嵌凭证），展示四难度成绩、RKS、推分建议等信息（PNG）。
     * @returns BinaryImage 图片（由 query format 决定）
     * @throws ApiError
     */
    public static renderSong({
        requestBody,
        format,
        template,
        width,
        webpQuality,
        webpLossless,
    }: {
        requestBody: RenderSongRequest,
        /**
         * 输出格式：png|jpeg|webp|svg（jpeg 接受别名 jpg），默认 png；未知值回落 png
         */
        format?: string,
        /**
         * SVG 模板 ID：对应 resources/templates/image/song/{id}.svg.jinja（不传则使用内置手写 SVG）
         */
        template?: string,
        /**
         * 目标宽度像素：按宽度同比例缩放
         */
        width?: number,
        /**
         * WebP 质量：1-100（仅在 format=webp 时有效，默认 80）
         */
        webpQuality?: number,
        /**
         * WebP 无损模式（仅在 format=webp 时有效，默认 false）
         */
        webpLossless?: boolean,
    }): CancelablePromise<BinaryImage> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/image/song',
            query: {
                'format': format,
                'template': template,
                'width': width,
                'webp_quality': webpQuality,
                'webp_lossless': webpLossless,
            },
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                400: `请求参数错误/认证缺失`,
                401: `Bearer 令牌无效或身份推导失败`,
                403: `用户已被封禁`,
                404: `歌曲未找到（unique search）`,
                409: `歌曲结果不唯一（unique search）`,
                422: `参数校验失败/渲染错误`,
                500: `服务器内部错误`,
            },
        });
    }
    /**
     * 验证图片签名（GET）
     * 通过 Query 参数 `svg` 传递 SVG 内容进行验证。
     * @returns VerifyResponse 验证结果
     * @throws ApiError
     */
    public static verifyImageGet({
        svg,
    }: {
        /**
         * 待验证的 SVG 字符串
         */
        svg?: string,
    }): CancelablePromise<VerifyResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/verify',
            query: {
                'svg': svg,
            },
        });
    }
    /**
     * 验证图片签名
     * 验证 SVG 中的 lilith-sig 签名，确保图片由本服务器合法生成。注意：该端点仅在 image.signing.public_verify=true 时注册；验证失败本身不视为错误，统一以 200 + valid=false 返回。
     * @returns VerifyResponse 验证结果（无论签名是否有效均返回 200，用 valid 字段区分）
     * @throws ApiError
     */
    public static verifyImage({
        requestBody,
    }: {
        requestBody: VerifyRequest,
    }): CancelablePromise<VerifyResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/verify',
            body: requestBody,
            mediaType: 'application/json',
            errors: {
                422: `请求体 JSON 无效`,
            },
        });
    }
    /**
     * 获取 Ed25519 公钥
     * 返回服务端 v4-beta 签名所用的 Ed25519 公钥。客户端拿到后即可脱离服务端独立验签。
     * @returns PublicKeyResponse 公钥信息
     * @throws ApiError
     */
    public static getPublicKey(): CancelablePromise<PublicKeyResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/verify/public-key',
        });
    }
}

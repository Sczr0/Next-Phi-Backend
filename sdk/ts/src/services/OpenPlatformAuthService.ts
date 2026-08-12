/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { DeveloperMeResponse } from '../models/DeveloperMeResponse';
import type { LogoutResponse } from '../models/LogoutResponse';
import type { CancelablePromise } from '../core/CancelablePromise';
import { OpenAPI } from '../core/OpenAPI';
import { request as __request } from '../core/request';
export class OpenPlatformAuthService {
    /**
     * GitHub OAuth 回调
     * 校验 OAuth state 与 code，换取 GitHub access token 并建立/复用开发者，随后签发 op_session 会话 Cookie 并 307 重定向到控制台。
     * @returns void
     * @throws ApiError
     */
    public static getGithubCallback({
        code,
        state,
        error,
        errorDescription,
    }: {
        /**
         * GitHub OAuth 授权码
         */
        code: string,
        /**
         * OAuth state（来自 /auth/github/login，单次有效）
         */
        state: string,
        /**
         * GitHub OAuth 错误标识（存在时直接返回 401）
         */
        error?: string,
        /**
         * GitHub OAuth 错误描述
         */
        errorDescription?: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/auth/github/callback',
            query: {
                'code': code,
                'state': state,
                'error': error,
                'error_description': errorDescription,
            },
            errors: {
                307: `登录成功并重定向控制台`,
                401: `state/code 无效或 GitHub 认证失败`,
                500: `服务端内部错误`,
                502: `上游网络错误（GitHub API 调用失败）`,
                504: `上游请求超时`,
            },
        });
    }
    /**
     * 发起 GitHub OAuth 登录
     * @returns void
     * @throws ApiError
     */
    public static getGithubLogin({
        redirect,
    }: {
        /**
         * 保留字段：前端跳转意图（当前未使用）
         */
        redirect?: string,
    }): CancelablePromise<void> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/auth/github/login',
            query: {
                'redirect': redirect,
            },
            errors: {
                307: `重定向到 GitHub 授权页`,
                500: `配置或服务初始化错误`,
            },
        });
    }
    /**
     * 开发者退出登录
     * @returns LogoutResponse 退出成功
     * @throws ApiError
     */
    public static postLogout(): CancelablePromise<LogoutResponse> {
        return __request(OpenAPI, {
            method: 'POST',
            url: '/auth/logout',
            errors: {
                500: `服务端内部错误`,
            },
        });
    }
    /**
     * 获取当前开发者登录信息
     * @returns DeveloperMeResponse 当前开发者信息
     * @throws ApiError
     */
    public static getMe(): CancelablePromise<DeveloperMeResponse> {
        return __request(OpenAPI, {
            method: 'GET',
            url: '/auth/me',
            errors: {
                401: `缺少或无效开发者会话`,
                500: `开放平台存储未初始化`,
            },
        });
    }
}

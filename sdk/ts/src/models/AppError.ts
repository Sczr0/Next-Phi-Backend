/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SaveProviderError } from './SaveProviderError';
import type { SearchError } from './SearchError';
/**
 * 应用统一错误类型
 */
export type AppError = ({
    /**
     * 授权未完成（轮询等待用户确认）
     */
    AuthPending: string;
} | {
    /**
     * 网络请求错误
     */
    Network: string;
} | {
    /**
     * JSON 解析错误
     */
    Json: string;
} | {
    /**
     * 认证失败 / 业务错误
     */
    Auth: string;
} | {
    /**
     * 保存处理错误
     */
    SaveHandlerError: string;
} | {
    /**
     * 图像渲染错误
     */
    ImageRendererError: string;
} | {
    /**
     * 参数校验错误
     */
    Validation: string;
} | {
    /**
     * 资源冲突（如别名占用）
     */
    Conflict: string;
} | {
    /**
     * 内部服务器错误
     */
    Internal: string;
} | {
    /**
     * 存档提供器错误
     */
    SaveProvider: SaveProviderError;
} | {
    /**
     * 搜索错误
     */
    Search: SearchError;
});


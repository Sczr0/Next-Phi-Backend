/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * 存档提供器错误类型
 */
export type SaveProviderError = ({
    /**
     * 网络请求错误
     */
    Network: string;
} | {
    /**
     * 认证失败
     */
    Auth: string;
} | {
    /**
     * 元数据解析错误
     */
    Metadata: string;
} | {
    /**
     * 缺少必需字段
     */
    MissingField: string;
} | {
    /**
     * 解密失败
     */
    Decrypt: string;
} | {
    /**
     * 完整性检查失败
     */
    Integrity: string;
} | 'InvalidPadding' | {
    /**
     * ZIP 解析错误
     */
    ZipError: string;
} | {
    /**
     * I/O 错误
     */
    Io: string;
} | {
    /**
     * JSON 解析错误
     */
    Json: string;
} | {
    /**
     * 不支持的功能
     */
    Unsupported: string;
} | {
    /**
     * 无效的响应
     */
    InvalidResponse: string;
} | 'Timeout' | 'InvalidHeader' | 'TagVerification' | {
    /**
     * 无效的凭据
     */
    InvalidCredentials: string;
});


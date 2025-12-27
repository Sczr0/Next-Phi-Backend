/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * RFC7807 风格的错误响应（Problem Details）。
 *
 * 设计目标：
 * - 让所有 API 错误返回结构化 JSON，便于 SDK/调用方稳定处理
 * - 与 OpenAPI 一致（content-type = application/problem+json）
 * - 允许在不破坏主结构的前提下扩展字段（如 requestId、字段级校验错误）
 */
export type ProblemDetails = {
    /**
     * 稳定的错误码，用于程序化处理。
     */
    code: string;
    /**
     * 人类可读的详细信息（尽量稳定，不建议依赖解析）。
     */
    detail?: string | null;
    /**
     * 可选：字段级校验错误（如表单/参数校验）。
     */
    errors?: any[] | null;
    /**
     * 可选：请求追踪 ID（如果后续加入 request-id middleware 可回填）。
     */
    requestId?: string | null;
    /**
     * HTTP 状态码（与响应 status 一致）。
     */
    status: number;
    /**
     * 简短标题，用于概括错误。
     */
    title: string;
    /**
     * 问题类型（URI）。若无更细分的类型，可使用 about:blank。
     */
    type: string;
};


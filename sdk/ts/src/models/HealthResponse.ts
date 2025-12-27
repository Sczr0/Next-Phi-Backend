/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
/**
 * 健康检查响应
 */
export type HealthResponse = {
    /**
     * 服务名称
     */
    service: string;
    /**
     * 服务状态
     */
    status: string;
    /**
     * 当前版本（Cargo package version）
     */
    version: string;
};


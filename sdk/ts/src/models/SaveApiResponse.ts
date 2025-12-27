/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SaveAndRksResponseDoc } from './SaveAndRksResponseDoc';
import type { SaveResponseDoc } from './SaveResponseDoc';
/**
 * oneOf 响应：仅解析存档，或解析存档并计算 RKS。
 */
export type SaveApiResponse = (SaveResponseDoc | SaveAndRksResponseDoc);


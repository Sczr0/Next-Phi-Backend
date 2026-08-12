/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type PublicKeyResponse = {
    /**
     * Ed25519 公钥（64 hex），未启用 v4 时为 null
     */
    publicKey?: string | null;
    /**
     * 签名协议版本（"v4-beta" 或 null）
     */
    version?: string | null;
};


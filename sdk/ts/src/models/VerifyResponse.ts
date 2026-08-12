/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type VerifyResponse = {
    contentHash?: string | null;
    edSig?: string | null;
    error?: string | null;
    merkleRoot?: string | null;
    nonce?: string | null;
    /**
     * 服务端 Ed25519 公钥（v4-beta），客户端可据此自行验签。
     */
    publicKey?: string | null;
    requestId?: string | null;
    scoreCount?: number | null;
    signedAt?: string | null;
    userHashPrefix?: string | null;
    valid: boolean;
    version?: string | null;
};


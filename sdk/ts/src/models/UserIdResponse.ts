/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type UserIdResponse = {
    /**
     * 去敏后的稳定用户 ID（32 位 hex，等价于 stats/leaderboard 使用的 user_hash）
     */
    userId: string;
    /**
     * 用于推导 user_id 的凭证类型（用于排查“为什么和以前不一致”）
     */
    userKind?: string | null;
};


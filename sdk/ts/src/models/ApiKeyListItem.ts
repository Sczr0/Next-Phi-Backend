/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type ApiKeyListItem = {
    createdAt: number;
    expiresAt?: number | null;
    id: string;
    keyLast4: string;
    keyMasked: string;
    keyPrefix: string;
    lastUsedAt?: number | null;
    lastUsedIp?: string | null;
    name: string;
    replacedByKeyId?: string | null;
    revokedAt?: number | null;
    scopes: Array<string>;
    status: string;
    usageCount: number;
};


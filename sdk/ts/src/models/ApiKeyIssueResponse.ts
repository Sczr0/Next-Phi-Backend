/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type ApiKeyIssueResponse = {
    createdAt: number;
    expiresAt?: number | null;
    id: string;
    keyLast4: string;
    keyMasked: string;
    keyPrefix: string;
    name: string;
    scopes: Array<string>;
    status: string;
    token: string;
};


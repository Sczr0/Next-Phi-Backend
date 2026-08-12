/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { SessionLogoutScope } from './SessionLogoutScope';
export type SessionLogoutResponse = {
    logoutBefore?: string | null;
    revokedJti: string;
    scope: SessionLogoutScope;
};


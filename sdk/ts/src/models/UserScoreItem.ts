/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type UserScoreItem = {
    /**
     * ACC 百分比（示例：98.50）
     */
    acc: number;
    /**
     * 难度（EZ/HD/IN/AT）
     */
    difficulty: string;
    /**
     * 分数（可选）
     */
    score?: number | null;
    /**
     * 歌曲 ID 或名称
     */
    song: string;
};


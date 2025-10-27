/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
export type ParsedSaveDoc = {
    /**
     * 游戏密钥块
     */
    game_key: any;
    /**
     * 进度信息（如金钱、拓展信息）
     */
    game_progress: any;
    /**
     * 结构化成绩（歌曲ID -> [四难度成绩]）
     */
    game_record: any;
    /**
     * 客户端设置
     */
    settings: any;
    /**
     * 解析自 summary 的关键摘要（如段位、RKS 等）
     */
    summaryParsed?: any;
    updatedAt?: string | null;
    /**
     * 用户基本信息
     */
    user: any;
};


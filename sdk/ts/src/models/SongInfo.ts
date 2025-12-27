/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { ChartConstants } from './ChartConstants';
/**
 * 单曲信息（来源：info/info.csv）
 */
export type SongInfo = {
    /**
     * 四难度定数（可为空）
     */
    chartConstants: ChartConstants;
    /**
     * 作曲者
     */
    composer: string;
    /**
     * 歌曲唯一 ID（与封面/定数等资源对应）
     */
    id: string;
    /**
     * 插画作者
     */
    illustrator: string;
    /**
     * 官方名称
     */
    name: string;
};


use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::startup::chart_loader::ChartConstants;

/// 单曲信息（来源：info/info.csv）
#[derive(Debug, Clone, utoipa::ToSchema, serde::Serialize)]
pub struct SongInfo {
    /// 歌曲唯一 ID（与封面/定数等资源对应）
    #[schema(example = "97f9466b2e77")]
    pub id: String,
    /// 官方名称
    #[schema(example = "Arcahv")]
    pub name: String,
    /// 作曲者
    #[schema(example = "Feryquitous")]
    pub composer: String,
    /// 插画作者
    #[schema(example = "Catrong")]
    pub illustrator: String,
    /// 四难度定数（可为空）
    pub chart_constants: ChartConstants,
}

/// 歌曲目录内存索引
#[derive(Debug, Default)]
pub struct SongCatalog {
    /// 通过歌曲 ID 直接索引
    pub by_id: HashMap<String, Arc<SongInfo>>,
    /// 通过官方歌曲名索引（可能重名，值为 Vec）
    pub by_name: HashMap<String, Vec<Arc<SongInfo>>>,
    /// 通过别名索引（可能重名，值为 Vec）
    pub by_nickname: HashMap<String, Vec<Arc<SongInfo>>>,
    search_cache_name_lower: Vec<(Arc<SongInfo>, String)>,
    search_cache_nick_lower: Vec<(String, String, Vec<Arc<SongInfo>>)>,
}

impl SongCatalog {
    /// 重建搜索缓存（在加载/更新索引后调用）。
    pub fn rebuild_search_cache(&mut self) {
        self.search_cache_name_lower.clear();
        self.search_cache_name_lower.reserve(self.by_id.len());
        for item in self.by_id.values() {
            self.search_cache_name_lower
                .push((Arc::clone(item), item.name.to_lowercase()));
        }

        self.search_cache_nick_lower.clear();
        self.search_cache_nick_lower.reserve(self.by_nickname.len());
        for (nick, list) in &self.by_nickname {
            self.search_cache_nick_lower.push((
                nick.clone(),
                nick.to_lowercase(),
                list.iter().map(Arc::clone).collect(),
            ));
        }
    }

    /// 通用查询：按 ID -> 官方名 -> 别名 的顺序查找。
    /// 当 ID 精确命中时，直接返回唯一结果；否则按以下优先级合并并去重：
    /// 1) 官方名：等于(忽略大小写) -> 前缀包含 -> 子串包含
    /// 2) 别名：等于(忽略大小写) -> 前缀包含 -> 子串包含
    pub fn search(&self, query: &str) -> Vec<Arc<SongInfo>> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }

        // 1) 先按 ID 精确命中（区分大小写）
        if let Some(info) = self.by_id.get(q) {
            return vec![Arc::clone(info)];
        }

        // 1.1) 再尝试按 ID 不区分大小写精确命中
        let q_lower = q.to_lowercase();
        if let Some((_id, info)) = self.by_id.iter().find(|(id, _)| id.eq_ignore_ascii_case(q)) {
            return vec![Arc::clone(info)];
        }

        // 2) 按官方名
        let mut result: Vec<Arc<SongInfo>> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new(); // 基于 id 去重

        // 预计算每首歌的 name_lower，避免在多轮遍历中反复 `to_lowercase()` 分配。
        // 使用预计算缓存，避免每次搜索都重复分配 `to_lowercase()`。

        // 官方名：等于（忽略大小写）
        for (item, _name_lower) in self.search_cache_name_lower.iter() {
            if item.name.eq_ignore_ascii_case(q) && seen.insert(item.id.as_str()) {
                result.push(Arc::clone(item));
            }
        }
        // 官方名：前缀包含（忽略大小写）
        for (item, name_lower) in self.search_cache_name_lower.iter() {
            if name_lower.starts_with(q_lower.as_str()) && seen.insert(item.id.as_str()) {
                result.push(Arc::clone(item));
            }
        }
        // 官方名：子串包含（忽略大小写）
        for (item, name_lower) in self.search_cache_name_lower.iter() {
            if name_lower.contains(q_lower.as_str()) && seen.insert(item.id.as_str()) {
                result.push(Arc::clone(item));
            }
        }

        // 3) 按别名索引：预计算 nick_lower，避免反复分配。
        // 别名使用预计算缓存，避免每次搜索都重复分配 `to_lowercase()`。

        // 按别名索引：等于（忽略大小写）
        for (nick, _nick_lower, list) in self.search_cache_nick_lower.iter() {
            if nick.eq_ignore_ascii_case(q) {
                for item in list.iter() {
                    if seen.insert(item.id.as_str()) {
                        result.push(Arc::clone(item));
                    }
                }
            }
        }
        // 别名：前缀包含（忽略大小写）
        for (_nick, nick_lower, list) in self.search_cache_nick_lower.iter() {
            if nick_lower.starts_with(q_lower.as_str()) {
                for item in list.iter() {
                    if seen.insert(item.id.as_str()) {
                        result.push(Arc::clone(item));
                    }
                }
            }
        }
        // 别名：子串包含（忽略大小写）
        for (_nick, nick_lower, list) in self.search_cache_nick_lower.iter() {
            if nick_lower.contains(q_lower.as_str()) {
                for item in list.iter() {
                    if seen.insert(item.id.as_str()) {
                        result.push(Arc::clone(item));
                    }
                }
            }
        }

        result
    }

    /// 强制唯一查询：当结果为 0/多于 1 时返回错误。
    pub fn search_unique(&self, query: &str) -> Result<Arc<SongInfo>, crate::error::SearchError> {
        use crate::error::SearchError;
        let results = self.search(query);
        match results.len() {
            0 => Err(SearchError::NotFound),
            1 => Ok(results.into_iter().next().unwrap()),
            _ => {
                // 将候选项转为拥有所有权的结构体副本，便于在错误中返回
                let candidates: Vec<SongInfo> = results
                    .into_iter()
                    .map(|arc| arc.as_ref().clone())
                    .collect();
                Err(SearchError::NotUnique { candidates })
            }
        }
    }

    /// 多关键词查询，支持 AND / OR、NOT（前缀 `-`）、短语（双引号），忽略大小写与前缀/子串匹配。
    pub fn search_multi(
        &self,
        query: &str,
        mode: SearchMode,
        options: SearchOptions,
    ) -> Vec<Arc<SongInfo>> {
        let tokens = parse_tokens(query, &options);
        if tokens.is_empty() {
            return Vec::new();
        }

        // 预备集合
        let all_ids: HashSet<&str> = self.by_id.keys().map(|s| s.as_str()).collect();

        // 计算每个 token 匹配到的歌曲 ID 集合
        let mut positives: Vec<HashSet<&str>> = Vec::new();
        let mut negatives: Vec<HashSet<&str>> = Vec::new();

        for t in &tokens {
            let matched = self.match_token(&t.text, &options);
            if t.is_exclude {
                negatives.push(matched);
            } else {
                positives.push(matched);
            }
        }

        // 合并正向匹配：AND 交集 / OR 并集
        let mut current: HashSet<&str> = if positives.is_empty() {
            // 若没有正向 token，则从全集开始，后续只做排除
            all_ids.clone()
        } else {
            match mode {
                SearchMode::And => {
                    let mut it = positives.into_iter();
                    let first = it.next().unwrap_or_default();
                    it.fold(first, |acc, set| &acc & &set)
                }
                SearchMode::Or => {
                    let mut acc: HashSet<&str> = HashSet::new();
                    for s in positives {
                        acc = &acc | &s;
                    }
                    acc
                }
            }
        };

        // 应用排除集
        for n in negatives {
            current = &current - &n;
        }

        // 转换为 Arc 并简单排序（粗略相关性）：精确等于 > 前缀 > 子串；官方名 > 别名 > 作曲 > ID
        let mut results: Vec<Arc<SongInfo>> = current
            .into_iter()
            .filter_map(|id| self.by_id.get(id).map(Arc::clone))
            .collect();

        results.sort_by(|a, b| {
            score_song(a, &tokens, &options, self)
                .cmp(&score_song(b, &tokens, &options, self))
                .reverse()
        });

        results
    }

    /// 针对单个 token 生成命中的歌曲 ID 集合（按任意字段）
    fn match_token<'a>(&'a self, token: &str, options: &SearchOptions) -> HashSet<&'a str> {
        let mut set: HashSet<&str> = HashSet::new();

        // 遍历所有歌曲：ID / 官方名 / 作曲者
        for s in self.by_id.values() {
            if field_match(&s.id, token, options)
                || field_match(&s.name, token, options)
                || field_match(&s.composer, token, options)
            {
                set.insert(s.id.as_str());
            }
        }

        // 别名匹配：匹配到键则将该键下的歌曲全部加入
        for (nick, list) in &self.by_nickname {
            if field_match(nick, token, options) {
                for s in list {
                    set.insert(s.id.as_str());
                }
            }
        }

        set
    }
}

/// AND/OR 查询模式
#[derive(Debug, Clone, Copy)]
pub enum SearchMode {
    And,
    Or,
}

/// 查询选项
#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    pub case_insensitive: bool,
    pub prefix: bool,
    pub substring: bool,
    pub pinyin: bool, // 预留（当前未实现）
    pub enable_not: bool,
    pub enable_phrase: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_insensitive: true,
            prefix: true,
            substring: true,
            pinyin: false,
            enable_not: true,
            enable_phrase: true,
        }
    }
}

/// 内部 token 结构
#[derive(Debug, Clone)]
struct Token {
    text: String,
    is_exclude: bool,
}

/// 将原始查询串解析为 tokens：支持双引号短语与负号排除
fn parse_tokens(input: &str, options: &SearchOptions) -> Vec<Token> {
    let s = input.trim();
    if s.is_empty() {
        return Vec::new();
    }

    let mut tokens: Vec<Token> = Vec::new();
    let mut buf = String::new();
    let mut in_quote = false;
    let mut is_exclude = false;

    let chars = s.chars().peekable();
    for ch in chars {
        match ch {
            '"' if options.enable_phrase => {
                if in_quote {
                    // 结束短语
                    let text = buf.trim().to_string();
                    if !text.is_empty() {
                        tokens.push(Token { text, is_exclude });
                    }
                    buf.clear();
                    in_quote = false;
                    is_exclude = false;
                } else {
                    // 开始短语；若短语前存在 '-'，视为排除短语
                    in_quote = true;
                    // 检查前一字符是否为减号（我们在空格处分词，只有紧邻双引号的 - 才能被当作排除）
                    // 这里简化：由 is_exclude 状态承接
                }
            }
            '-' if !in_quote && options.enable_not => {
                // 仅当当前缓冲区为空时，视为排除标记
                if buf.trim().is_empty() {
                    is_exclude = true;
                } else {
                    buf.push(ch);
                }
            }
            c if c.is_whitespace() && !in_quote => {
                let text = buf.trim().to_string();
                if !text.is_empty() {
                    tokens.push(Token { text, is_exclude });
                }
                buf.clear();
                is_exclude = false;
            }
            _ => buf.push(ch),
        }
    }
    let text = buf.trim().to_string();
    if !text.is_empty() {
        tokens.push(Token { text, is_exclude });
    }

    tokens
}

/// 字段匹配：等于（忽略大小写）/ 前缀 / 子串
fn field_match(field: &str, token: &str, options: &SearchOptions) -> bool {
    if field.is_empty() || token.is_empty() {
        return false;
    }
    if options.case_insensitive {
        let f = field.to_lowercase();
        let t = token.to_lowercase();
        if f == t {
            return true;
        }
        if options.prefix && f.starts_with(&t) {
            return true;
        }
        if options.substring && f.contains(&t) {
            return true;
        }
        false
    } else {
        if field == token {
            return true;
        }
        if options.prefix && field.starts_with(token) {
            return true;
        }
        if options.substring && field.contains(token) {
            return true;
        }
        false
    }
}

/// 简易打分：精确等于 > 前缀 > 子串；官方名 > 别名 > 作曲 > ID；多 token 累加
fn score_song(
    song: &Arc<SongInfo>,
    tokens: &[Token],
    options: &SearchOptions,
    catalog: &SongCatalog,
) -> i32 {
    let mut score = 0i32;
    for t in tokens {
        if t.is_exclude {
            continue;
        }
        let tok = t.text.as_str();

        // 官方名
        score += score_field(&song.name, tok, options, 100, 80, 60);
        // 别名（任一匹配即可按权重计分）
        let mut nick_scored = false;
        for nick in catalog.by_nickname.keys() {
            if !nick_scored && field_match(nick, tok, options) {
                score += 70; // 近似于官方名下一档
                nick_scored = true;
            }
        }
        // 作曲者
        score += score_field(&song.composer, tok, options, 50, 35, 20);
        // ID
        score += score_field(&song.id, tok, options, 40, 25, 10);
    }
    score
}

fn score_field(
    field: &str,
    token: &str,
    options: &SearchOptions,
    eq_w: i32,
    pre_w: i32,
    sub_w: i32,
) -> i32 {
    if field.is_empty() {
        return 0;
    }
    if options.case_insensitive {
        let f = field.to_lowercase();
        let t = token.to_lowercase();
        if f == t {
            return eq_w;
        }
        if options.prefix && f.starts_with(&t) {
            return pre_w;
        }
        if options.substring && f.contains(&t) {
            return sub_w;
        }
        0
    } else {
        if field == token {
            return eq_w;
        }
        if options.prefix && field.starts_with(token) {
            return pre_w;
        }
        if options.substring && field.contains(token) {
            return sub_w;
        }
        0
    }
}

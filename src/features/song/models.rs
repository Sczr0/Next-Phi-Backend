use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::startup::chart_loader::ChartConstants;

/// 单曲信息（来源：info/info.csv）
#[derive(Debug, Clone, utoipa::ToSchema, serde::Serialize)]
#[serde(rename_all = "camelCase")]
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

/// 搜索候选预览（用于歧义查询时的提示）。
#[derive(Debug, Clone, utoipa::ToSchema, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongCandidatePreview {
    pub id: String,
    pub name: String,
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

#[derive(Debug, Clone, Copy)]
enum SearchMatchKind {
    NameEquals,
    NamePrefix,
    NameContains,
    NickEquals,
    NickPrefix,
    NickContains,
}

impl SongCatalog {
    fn score_match(
        kind: SearchMatchKind,
        q_lower: &str,
        hay_lower: &str,
        pos: Option<usize>,
    ) -> i32 {
        // 说明：
        // - 这是“用于排序/挑选候选预览”的粗略匹配值（越大越相关）。
        // - 基础分严格遵循 search() 的合并顺序：官方名 > 别名；等于 > 前缀 > 子串。
        // - 额外分用于在同一匹配类型内做更友好的排序（更短、更靠前的命中更优）。

        let q_len = q_lower.len() as i32;
        let hay_len = hay_lower.len() as i32;
        let extra_len_penalty = (hay_len - q_len).clamp(0, 200); // 越长越不相关（上限避免过度影响）
        let pos_bonus = pos
            .map(|p| (200_i32 - (p as i32).min(200)).max(0))
            .unwrap_or(0); // 命中越靠前越优

        match kind {
            SearchMatchKind::NameEquals => 6000,
            SearchMatchKind::NamePrefix => 5000 + q_len * 5 - extra_len_penalty,
            SearchMatchKind::NameContains => 4000 + pos_bonus - extra_len_penalty,
            SearchMatchKind::NickEquals => 3000,
            SearchMatchKind::NickPrefix => 2000 + q_len * 5 - extra_len_penalty,
            SearchMatchKind::NickContains => 1000 + pos_bonus - extra_len_penalty,
        }
    }

    fn visit_search_matches<F>(&self, query: &str, mut on_match: F)
    where
        F: FnMut(&Arc<SongInfo>, SearchMatchKind, i32),
    {
        let q = query.trim();
        if q.is_empty() {
            return;
        }

        let q_lower = q.to_lowercase();
        let mut seen: HashSet<&str> = HashSet::new(); // 基于 id 去重

        // 官方名：等于（忽略大小写）
        for (item, name_lower) in self.search_cache_name_lower.iter() {
            if item.name.eq_ignore_ascii_case(q) && seen.insert(item.id.as_str()) {
                on_match(
                    item,
                    SearchMatchKind::NameEquals,
                    Self::score_match(SearchMatchKind::NameEquals, &q_lower, name_lower, Some(0)),
                );
            }
        }
        // 官方名：前缀包含（忽略大小写）
        for (item, name_lower) in self.search_cache_name_lower.iter() {
            if name_lower.starts_with(q_lower.as_str()) && seen.insert(item.id.as_str()) {
                on_match(
                    item,
                    SearchMatchKind::NamePrefix,
                    Self::score_match(SearchMatchKind::NamePrefix, &q_lower, name_lower, Some(0)),
                );
            }
        }
        // 官方名：子串包含（忽略大小写）
        for (item, name_lower) in self.search_cache_name_lower.iter() {
            if let Some(pos) = name_lower.find(q_lower.as_str())
                && seen.insert(item.id.as_str())
            {
                on_match(
                    item,
                    SearchMatchKind::NameContains,
                    Self::score_match(
                        SearchMatchKind::NameContains,
                        &q_lower,
                        name_lower,
                        Some(pos),
                    ),
                );
            }
        }

        // 别名：等于（忽略大小写）
        for (nick, nick_lower, list) in self.search_cache_nick_lower.iter() {
            if nick.eq_ignore_ascii_case(q) {
                let score =
                    Self::score_match(SearchMatchKind::NickEquals, &q_lower, nick_lower, Some(0));
                for item in list.iter() {
                    if seen.insert(item.id.as_str()) {
                        on_match(item, SearchMatchKind::NickEquals, score);
                    }
                }
            }
        }
        // 别名：前缀包含（忽略大小写）
        for (_nick, nick_lower, list) in self.search_cache_nick_lower.iter() {
            if nick_lower.starts_with(q_lower.as_str()) {
                let score =
                    Self::score_match(SearchMatchKind::NickPrefix, &q_lower, nick_lower, Some(0));
                for item in list.iter() {
                    if seen.insert(item.id.as_str()) {
                        on_match(item, SearchMatchKind::NickPrefix, score);
                    }
                }
            }
        }
        // 别名：子串包含（忽略大小写）
        for (_nick, nick_lower, list) in self.search_cache_nick_lower.iter() {
            if let Some(pos) = nick_lower.find(q_lower.as_str()) {
                let score = Self::score_match(
                    SearchMatchKind::NickContains,
                    &q_lower,
                    nick_lower,
                    Some(pos),
                );
                for item in list.iter() {
                    if seen.insert(item.id.as_str()) {
                        on_match(item, SearchMatchKind::NickContains, score);
                    }
                }
            }
        }
    }

    /// 分页查询：返回当前页 items 与 total（总命中数）。
    ///
    /// 设计目标：避免 HTTP 层“先全量构建 Vec 再切片”的不必要分配；同时保持与 `search()` 一致的排序语义。
    pub fn search_page(&self, query: &str, offset: u32, limit: u32) -> (Vec<Arc<SongInfo>>, usize) {
        let q = query.trim();
        if q.is_empty() {
            return (Vec::new(), 0);
        }

        // 1) 先按 ID 精确命中（区分大小写）
        if let Some(info) = self.by_id.get(q) {
            let items = if offset == 0 && limit > 0 {
                vec![Arc::clone(info)]
            } else {
                Vec::new()
            };
            return (items, 1);
        }

        // 1.1) 再尝试按 ID 不区分大小写精确命中
        if let Some((_id, info)) = self.by_id.iter().find(|(id, _)| id.eq_ignore_ascii_case(q)) {
            let items = if offset == 0 && limit > 0 {
                vec![Arc::clone(info)]
            } else {
                Vec::new()
            };
            return (items, 1);
        }

        let start = offset as usize;
        let max_take = limit as usize;
        let end = start.saturating_add(max_take);

        let mut items: Vec<Arc<SongInfo>> = Vec::new();
        let mut total: usize = 0;

        if max_take == 0 {
            // 语义保持：limit=0 视为不返回 items，但 total 仍应正确（调用方通常已校验 limit>=1）。
            self.visit_search_matches(q, |_item, _kind, _score| {
                total = total.saturating_add(1);
            });
            return (items, total);
        }

        self.visit_search_matches(q, |item, _kind, _score| {
            if total >= start && total < end {
                items.push(Arc::clone(item));
            }
            total = total.saturating_add(1);
        });

        (items, total)
    }

    /// 重建搜索缓存（在加载/更新索引后调用）。
    pub fn rebuild_search_cache(&mut self) {
        // 注意：HashMap 迭代顺序不稳定。这里将缓存构建为“稳定排序”的 Vec，确保搜索结果顺序跨进程一致。

        // 1) name cache：按 name_lower，再按 id 排序
        let mut name_entries: Vec<(Arc<SongInfo>, String)> = self
            .by_id
            .values()
            .map(|item| (Arc::clone(item), item.name.to_lowercase()))
            .collect();
        name_entries.sort_by(|(a_item, a_lower), (b_item, b_lower)| {
            a_lower.cmp(b_lower).then_with(|| a_item.id.cmp(&b_item.id))
        });
        self.search_cache_name_lower = name_entries;

        // 2) nickname cache：先按 nick_lower/nick 排序；每个 nick 下的歌曲也按 name_lower/id 稳定排序
        let mut nick_entries: Vec<(String, String, Vec<Arc<SongInfo>>)> =
            Vec::with_capacity(self.by_nickname.len());
        for (nick, list) in &self.by_nickname {
            // 为避免在 sort 比较中反复分配 to_lowercase，这里预先计算 key
            let mut sortable: Vec<(String, String, Arc<SongInfo>)> = list
                .iter()
                .map(|s| (s.name.to_lowercase(), s.id.clone(), Arc::clone(s)))
                .collect();
            sortable.sort_by(|(a_name, a_id, _), (b_name, b_id, _)| {
                a_name.cmp(b_name).then_with(|| a_id.cmp(b_id))
            });
            let sorted_list: Vec<Arc<SongInfo>> = sortable.into_iter().map(|(_, _, s)| s).collect();

            nick_entries.push((nick.clone(), nick.to_lowercase(), sorted_list));
        }
        nick_entries.sort_by(|(a_nick, a_lower, _), (b_nick, b_lower, _)| {
            a_lower.cmp(b_lower).then_with(|| a_nick.cmp(b_nick))
        });
        self.search_cache_nick_lower = nick_entries;
    }

    /// 通用查询：按 ID -> 官方名 -> 别名 的顺序查找。
    /// 当 ID 精确命中时，直接返回唯一结果；否则按以下优先级合并并去重：
    /// 1) 官方名：等于(忽略大小写) -> 前缀包含 -> 子串包含
    /// 2) 别名：等于(忽略大小写) -> 前缀包含 -> 子串包含
    pub fn search(&self, query: &str) -> Vec<Arc<SongInfo>> {
        let (items, _total) = self.search_page(query, 0, u32::MAX);
        items
    }

    /// 强制唯一查询：当结果为 0/多于 1 时返回错误。
    pub fn search_unique(&self, query: &str) -> Result<Arc<SongInfo>, crate::error::SearchError> {
        use std::cmp::Ordering;

        use crate::error::SearchError;

        const CANDIDATE_PREVIEW_LIMIT: usize = 10;

        let q = query.trim();
        if q.is_empty() {
            return Err(SearchError::NotFound);
        }

        // 1) 先按 ID 精确命中（区分大小写）
        if let Some(info) = self.by_id.get(q) {
            return Ok(Arc::clone(info));
        }

        // 1.1) 再尝试按 ID 不区分大小写精确命中
        if let Some((_id, info)) = self.by_id.iter().find(|(id, _)| id.eq_ignore_ascii_case(q)) {
            return Ok(Arc::clone(info));
        }

        #[derive(Clone)]
        struct CandidateScored {
            score: i32,
            name_lower: String,
            song: Arc<SongInfo>,
        }

        impl CandidateScored {
            /// 比较“哪个更好”：分数更高更好；同分时 name_lower 更小更好；再同分时 id 更小更好。
            fn cmp_better(&self, other: &Self) -> Ordering {
                self.score
                    .cmp(&other.score)
                    .then_with(|| other.name_lower.cmp(&self.name_lower))
                    .then_with(|| other.song.id.cmp(&self.song.id))
            }
        }

        fn worst_index(items: &[CandidateScored]) -> usize {
            let mut worst = 0_usize;
            for (i, c) in items.iter().enumerate().skip(1) {
                if c.cmp_better(&items[worst]) == Ordering::Less {
                    worst = i;
                }
            }
            worst
        }

        let mut total: usize = 0;
        let mut only: Option<Arc<SongInfo>> = None;
        let mut top: Vec<CandidateScored> = Vec::new();

        // 说明：此处会遍历所有可能结果以计算匹配值与 total，但只保留受控数量的候选预览，避免“无收益克隆”。
        self.visit_search_matches(q, |item, _kind, score| {
            total = total.saturating_add(1);
            if total == 1 {
                only = Some(Arc::clone(item));
            }

            if top.len() < CANDIDATE_PREVIEW_LIMIT {
                top.push(CandidateScored {
                    score,
                    name_lower: item.name.to_lowercase(),
                    song: Arc::clone(item),
                });
                return;
            }

            let worst = worst_index(&top);
            let worst_score = top[worst].score;
            if score < worst_score {
                return;
            }

            // 只有可能入榜时才做额外分配（name_lower 与 Arc clone）。
            let cand = CandidateScored {
                score,
                name_lower: item.name.to_lowercase(),
                song: Arc::clone(item),
            };
            if cand.cmp_better(&top[worst]) == Ordering::Greater {
                top[worst] = cand;
            }
        });

        match total {
            0 => Err(SearchError::NotFound),
            1 => Ok(only.expect("total==1 implies only exists")),
            _ => {
                top.sort_by(|a, b| a.cmp_better(b).reverse());
                let candidates: Vec<SongCandidatePreview> = top
                    .into_iter()
                    .map(|c| SongCandidatePreview {
                        id: c.song.id.clone(),
                        name: c.song.name.clone(),
                    })
                    .collect();

                let total_u32 = u32::try_from(total).unwrap_or(u32::MAX);
                Err(SearchError::NotUnique {
                    total: total_u32,
                    candidates,
                })
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

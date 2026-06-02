use chrono::NaiveDate;

use crate::error::AppError;

pub(super) fn normalize_top_per_day(top: Option<i64>) -> Result<i64, AppError> {
    const DEFAULT_TOP: i64 = 200;
    const MAX_TOP: i64 = 200;
    match top {
        None => Ok(DEFAULT_TOP),
        Some(v) if v <= 0 => Err(AppError::Validation("top 必须为正整数".into())),
        Some(v) => Ok(v.min(MAX_TOP)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LatencyBucket {
    Day,
    Week,
    Month,
}

impl LatencyBucket {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            LatencyBucket::Day => "day",
            LatencyBucket::Week => "week",
            LatencyBucket::Month => "month",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LatencyAggFilters<'a> {
    pub(super) feature: Option<&'a str>,
    pub(super) route: Option<&'a str>,
    pub(super) method: Option<&'a str>,
}

pub(super) fn parse_latency_bucket(s: Option<&str>) -> Result<LatencyBucket, AppError> {
    match s.unwrap_or("day") {
        "day" => Ok(LatencyBucket::Day),
        "week" => Ok(LatencyBucket::Week),
        "month" => Ok(LatencyBucket::Month),
        other => Err(AppError::Validation(format!(
            "bucket 无效（可选：day/week/month）：{other}"
        ))),
    }
}

#[derive(Debug, Clone)]
pub(super) struct DateBucket {
    pub(super) label: String,
    pub(super) start: NaiveDate,
    pub(super) end: NaiveDate,
}

#[derive(Default, Clone, Copy)]
pub(super) struct IncludeFlags {
    pub(super) routes: bool,
    pub(super) methods: bool,
    pub(super) status_codes: bool,
    pub(super) instances: bool,
    pub(super) actions: bool,
    pub(super) latency: bool,
    pub(super) unique_ips: bool,
    pub(super) user_kinds: bool,
}

impl IncludeFlags {
    #[cfg(test)]
    pub(super) fn any(self) -> bool {
        self.routes
            || self.methods
            || self.status_codes
            || self.instances
            || self.actions
            || self.latency
            || self.unique_ips
            || self.user_kinds
    }

    #[cfg(test)]
    pub(super) fn any_http(self) -> bool {
        self.routes || self.methods || self.status_codes || self.latency || self.unique_ips
    }
}

pub(super) fn parse_include_flags(include: Option<&str>) -> IncludeFlags {
    let Some(s) = include else {
        return IncludeFlags::default();
    };

    let mut flags = IncludeFlags::default();
    for raw in s.split([',', ';', ' ', '\t', '\n']) {
        let t = raw.trim().to_ascii_lowercase();
        if t.is_empty() {
            continue;
        }
        if t == "all" {
            return IncludeFlags {
                routes: true,
                methods: true,
                status_codes: true,
                instances: true,
                actions: true,
                latency: true,
                unique_ips: true,
                user_kinds: true,
            };
        }
        match t.as_str() {
            "routes" | "route" => flags.routes = true,
            "methods" | "method" => flags.methods = true,
            "status" | "statuses" | "status_codes" | "statuscodes" => flags.status_codes = true,
            "instances" | "instance" => flags.instances = true,
            "actions" | "action" => flags.actions = true,
            "latency" => flags.latency = true,
            "unique_ips" | "uniqueip" | "uniqueips" | "ips" => flags.unique_ips = true,
            "user_kinds" | "userkind" | "userkinds" | "kinds" | "by_kind" => {
                flags.user_kinds = true;
            }
            _ => {}
        }
    }
    flags
}

pub(super) fn normalize_top(top: Option<i64>) -> Result<i64, AppError> {
    const DEFAULT_TOP: i64 = 20;
    const MAX_TOP: i64 = 200;
    match top {
        None => Ok(DEFAULT_TOP),
        Some(v) if v <= 0 => Err(AppError::Validation("top 必须为正整数".into())),
        Some(v) => Ok(v.min(MAX_TOP)),
    }
}

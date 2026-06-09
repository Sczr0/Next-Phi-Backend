use chrono::{Datelike, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone};

use crate::error::AppError;

pub(super) fn convert_tz(ts_rfc3339: &str, tz: chrono_tz::Tz) -> Option<String> {
    let dt = chrono::DateTime::parse_from_rfc3339(ts_rfc3339).ok()?;
    let as_utc = dt.with_timezone(&chrono::Utc);
    Some(as_utc.with_timezone(&tz).to_rfc3339())
}

pub(super) fn parse_ymd(s: &str, field: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| {
        AppError::Validation(format!("{field} 日期无效（期望 YYYY-MM-DD）: {s} ({e})"))
    })
}

pub(super) fn validate_date_range(start: NaiveDate, end: NaiveDate) -> Result<(), AppError> {
    if end < start {
        return Err(AppError::Validation("end 不能早于 start".into()));
    }
    const MAX_DAYS: i64 = 366;
    let days = (end - start).num_days() + 1;
    if days > MAX_DAYS {
        return Err(AppError::Validation(format!(
            "日期范围过大：{days} 天（上限 {MAX_DAYS} 天）"
        )));
    }
    Ok(())
}

pub(super) fn sqlite_minutes_modifier(offset_minutes: i32) -> String {
    format!("{offset_minutes:+} minutes")
}

pub(super) fn fixed_offset_minutes_for_range(
    tz: chrono_tz::Tz,
    start: NaiveDate,
    end: NaiveDate,
) -> Option<i32> {
    let mut cur = start;
    let mut offset: Option<i32> = None;
    while cur <= end {
        // noon 一般不会处于 DST 的 ambiguous/none 区间，作为稳定采样点
        let local_noon = NaiveDateTime::new(cur, NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        let dt = match tz.from_local_datetime(&local_noon) {
            LocalResult::Single(v) => v,
            LocalResult::Ambiguous(a, _) => a,
            LocalResult::None => tz.from_utc_datetime(&local_noon),
        };
        let off_secs = dt.offset().fix().local_minus_utc();
        let off_min = off_secs / 60;
        match offset {
            None => offset = Some(off_min),
            Some(prev) if prev == off_min => {}
            Some(_) => return None,
        }
        cur += chrono::Duration::days(1);
    }
    offset
}

pub(super) fn week_start_monday(d: NaiveDate) -> NaiveDate {
    let delta = i64::from(d.weekday().num_days_from_monday());
    d - chrono::Duration::days(delta)
}

pub(super) fn month_start_day1(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).expect("valid ymd")
}

pub(super) fn next_month_start(d: NaiveDate) -> NaiveDate {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("valid ymd")
}

pub(super) fn resolve_timezone(
    config_tz: &str,
    query_tz: Option<&str>,
) -> Result<(String, chrono_tz::Tz), AppError> {
    if let Some(name) = query_tz {
        let tz = name
            .parse::<chrono_tz::Tz>()
            .map_err(|_| AppError::Validation(format!("timezone 无效: {name}")))?;
        return Ok((name.to_string(), tz));
    }
    match config_tz.parse::<chrono_tz::Tz>() {
        Ok(tz) => Ok((config_tz.to_string(), tz)),
        Err(_) => Ok(("Asia/Shanghai".to_string(), chrono_tz::Asia::Shanghai)),
    }
}

pub(super) fn parse_date_bound_utc(
    date_ymd: &str,
    tz: chrono_tz::Tz,
    is_end: bool,
) -> Result<String, AppError> {
    let date = NaiveDate::parse_from_str(date_ymd, "%Y-%m-%d").map_err(|e| {
        AppError::Validation(format!("日期无效（期望 YYYY-MM-DD）: {date_ymd} ({e})"))
    })?;
    let time = if is_end {
        NaiveTime::from_hms_opt(23, 59, 59).unwrap()
    } else {
        NaiveTime::from_hms_opt(0, 0, 0).unwrap()
    };
    let ndt = NaiveDateTime::new(date, time);

    let local = match tz.from_local_datetime(&ndt) {
        LocalResult::Single(v) => v,
        LocalResult::Ambiguous(a, b) => {
            if is_end {
                b
            } else {
                a
            }
        }
        LocalResult::None => chrono::Utc.from_utc_datetime(&ndt).with_timezone(&tz),
    };
    Ok(local.with_timezone(&chrono::Utc).to_rfc3339())
}

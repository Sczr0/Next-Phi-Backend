//! 本地管理员命令行工具：
//! - 查看排行榜完整 user_hash
//! - 扫描可疑用户（返回完整 user_hash，便于直接封禁）
//! - 查询/设置全局用户状态（含 ban / unban 快捷命令）

use std::cmp::Ordering;
use std::env;
use std::fmt::{Display, Formatter};
use std::time::Duration;

use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:3939/api/v2";
const DEFAULT_ADMIN_TOKEN_ENV: &str = "PHI_ADMIN_TOKEN";
const DEFAULT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_USERS_PAGE_SIZE: i64 = 50;
const DEFAULT_SUSPICIOUS_MIN_SCORE: f64 = 0.6;
const DEFAULT_SUSPICIOUS_SCAN_PAGES: i64 = 5;
const DEFAULT_SUSPICIOUS_PAGE_SIZE: i64 = 100;
const DEFAULT_SUSPICIOUS_LIMIT: usize = 200;

#[derive(Debug, Clone)]
struct RuntimeDefaults {
    base_url: String,
    admin_token: Option<String>,
}

#[derive(Debug, Clone)]
struct Args {
    help: bool,
    json: bool,
    base_url: Option<String>,
    token: Option<String>,
    token_env: String,
    timeout_secs: u64,
    cmd: Option<Command>,
}

#[derive(Debug, Clone)]
enum Command {
    Help,
    Users(UsersCmd),
    Suspicious(SuspiciousCmd),
    Status(UserHashCmd),
    SetStatus(SetStatusCmd),
    Ban(UserHashReasonCmd),
    Unban(UserHashReasonCmd),
}

#[derive(Debug, Clone)]
struct UsersCmd {
    page: i64,
    page_size: i64,
    status: Option<String>,
    alias: Option<String>,
}

#[derive(Debug, Clone)]
struct SuspiciousCmd {
    min_score: f64,
    scan_pages: i64,
    page_size: i64,
    limit: usize,
    status: Option<String>,
    alias: Option<String>,
}

#[derive(Debug, Clone)]
struct UserHashCmd {
    user_hash: String,
}

#[derive(Debug, Clone)]
struct UserHashReasonCmd {
    user_hash: String,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
struct SetStatusCmd {
    user_hash: String,
    status: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AdminLeaderboardUserItem {
    user_hash: String,
    alias: Option<String>,
    score: f64,
    suspicion: f64,
    is_hidden: bool,
    status: String,
    updated_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminLeaderboardUsersResponse {
    items: Vec<AdminLeaderboardUserItem>,
    total: i64,
    page: i64,
    page_size: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserStatusResponse {
    user_hash: String,
    status: String,
    reason: Option<String>,
    updated_by: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminSetUserStatusRequest {
    user_hash: String,
    status: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProblemDetails {
    title: String,
    status: u16,
    detail: Option<String>,
    code: String,
    request_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuspiciousScanResult {
    min_score: f64,
    scanned_pages: i64,
    page_size: i64,
    returned: usize,
    items: Vec<AdminLeaderboardUserItem>,
}

#[derive(Debug)]
enum CliError {
    Args(String),
    Config(String),
    Network(String),
    Decode(String),
    Api {
        status: u16,
        code: String,
        detail: String,
        request_id: Option<String>,
    },
}

impl Display for CliError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Args(msg) => write!(f, "参数错误: {msg}"),
            CliError::Config(msg) => write!(f, "配置错误: {msg}"),
            CliError::Network(msg) => write!(f, "网络错误: {msg}"),
            CliError::Decode(msg) => write!(f, "响应解析失败: {msg}"),
            CliError::Api {
                status,
                code,
                detail,
                request_id,
            } => {
                if let Some(rid) = request_id {
                    write!(
                        f,
                        "接口错误: status={status} code={code} detail={detail} requestId={rid}"
                    )
                } else {
                    write!(f, "接口错误: status={status} code={code} detail={detail}")
                }
            }
        }
    }
}

impl std::error::Error for CliError {}

struct AdminApi {
    client: Client,
    base_url: String,
    admin_token: String,
}

#[derive(Debug, Default, Clone)]
struct UsersQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    status: Option<String>,
    alias: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let args = Args::parse(env::args().skip(1).collect())?;
    if args.help || args.cmd.is_none() {
        print_help();
        return Ok(());
    }

    if matches!(args.cmd, Some(Command::Help)) {
        print_help();
        return Ok(());
    }

    let defaults = load_runtime_defaults();
    let base_url = args
        .base_url
        .unwrap_or(defaults.base_url)
        .trim_end_matches('/')
        .to_string();
    let admin_token = resolve_admin_token(args.token, &args.token_env, defaults.admin_token)?;

    let api = AdminApi::new(base_url, admin_token, args.timeout_secs)?;

    let outcome = match args.cmd.expect("cmd checked above") {
        Command::Help => unreachable!("help command handled before token resolution"),
        Command::Users(cmd) => run_users(&api, cmd, args.json).await,
        Command::Suspicious(cmd) => run_suspicious(&api, cmd, args.json).await,
        Command::Status(cmd) => run_status(&api, cmd, args.json).await,
        Command::SetStatus(cmd) => run_set_status(&api, cmd, args.json).await,
        Command::Ban(cmd) => {
            let set = SetStatusCmd {
                user_hash: cmd.user_hash,
                status: "banned".to_string(),
                reason: cmd.reason,
            };
            run_set_status(&api, set, args.json).await
        }
        Command::Unban(cmd) => {
            let set = SetStatusCmd {
                user_hash: cmd.user_hash,
                status: "active".to_string(),
                reason: cmd.reason,
            };
            run_set_status(&api, set, args.json).await
        }
    };

    if let Err(err) = outcome {
        eprintln!("{err}");
        std::process::exit(2);
    }
    Ok(())
}

impl Args {
    fn parse(argv: Vec<String>) -> Result<Self, CliError> {
        let mut help = false;
        let mut json = false;
        let mut base_url = None;
        let mut token = None;
        let mut token_env = DEFAULT_ADMIN_TOKEN_ENV.to_string();
        let mut timeout_secs = DEFAULT_TIMEOUT_SECS;
        let mut idx = 0usize;

        while idx < argv.len() {
            let a = argv[idx].as_str();
            match a {
                "-h" | "--help" => {
                    help = true;
                    idx += 1;
                }
                "--json" => {
                    json = true;
                    idx += 1;
                }
                "--base-url" => {
                    idx += 1;
                    base_url = Some(
                        argv.get(idx)
                            .ok_or_else(|| CliError::Args("缺少 --base-url 的值".to_string()))?
                            .to_string(),
                    );
                    idx += 1;
                }
                "--token" => {
                    idx += 1;
                    token = Some(
                        argv.get(idx)
                            .ok_or_else(|| CliError::Args("缺少 --token 的值".to_string()))?
                            .to_string(),
                    );
                    idx += 1;
                }
                "--token-env" => {
                    idx += 1;
                    token_env = argv
                        .get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --token-env 的值".to_string()))?
                        .to_string();
                    idx += 1;
                }
                "--timeout-secs" => {
                    idx += 1;
                    let raw = argv
                        .get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --timeout-secs 的值".to_string()))?;
                    timeout_secs = parse_u64(raw, "--timeout-secs")?;
                    idx += 1;
                }
                _ => break,
            }
        }

        let cmd = if idx >= argv.len() {
            None
        } else {
            Some(parse_command(&argv[idx], &argv[(idx + 1)..])?)
        };

        Ok(Self {
            help,
            json,
            base_url,
            token,
            token_env,
            timeout_secs,
            cmd,
        })
    }
}

fn parse_command(name: &str, rest: &[String]) -> Result<Command, CliError> {
    match name {
        "users" => parse_users_cmd(rest).map(Command::Users),
        "suspicious" => parse_suspicious_cmd(rest).map(Command::Suspicious),
        "status" => parse_status_cmd(rest).map(Command::Status),
        "set-status" => parse_set_status_cmd(rest).map(Command::SetStatus),
        "ban" => parse_user_hash_reason_cmd(rest, "ban").map(Command::Ban),
        "unban" => parse_user_hash_reason_cmd(rest, "unban").map(Command::Unban),
        "help" => Ok(Command::Help),
        _ => Err(CliError::Args(format!("未知命令: {name}"))),
    }
}

fn parse_users_cmd(rest: &[String]) -> Result<UsersCmd, CliError> {
    let mut cmd = UsersCmd {
        page: 1,
        page_size: DEFAULT_USERS_PAGE_SIZE,
        status: None,
        alias: None,
    };

    let mut idx = 0usize;
    while idx < rest.len() {
        match rest[idx].as_str() {
            "--page" => {
                idx += 1;
                cmd.page = parse_i64(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --page 的值".to_string()))?,
                    "--page",
                )?
                .max(1);
                idx += 1;
            }
            "--page-size" => {
                idx += 1;
                cmd.page_size = parse_i64(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --page-size 的值".to_string()))?,
                    "--page-size",
                )?
                .clamp(1, 200);
                idx += 1;
            }
            "--status" => {
                idx += 1;
                cmd.status = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --status 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            "--alias" => {
                idx += 1;
                cmd.alias = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --alias 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            unknown => {
                return Err(CliError::Args(format!("users 不支持参数: {unknown}")));
            }
        }
    }
    Ok(cmd)
}

fn parse_suspicious_cmd(rest: &[String]) -> Result<SuspiciousCmd, CliError> {
    let mut cmd = SuspiciousCmd {
        min_score: DEFAULT_SUSPICIOUS_MIN_SCORE,
        scan_pages: DEFAULT_SUSPICIOUS_SCAN_PAGES,
        page_size: DEFAULT_SUSPICIOUS_PAGE_SIZE,
        limit: DEFAULT_SUSPICIOUS_LIMIT,
        status: None,
        alias: None,
    };

    let mut idx = 0usize;
    while idx < rest.len() {
        match rest[idx].as_str() {
            "--min-score" => {
                idx += 1;
                cmd.min_score = parse_f64(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --min-score 的值".to_string()))?,
                    "--min-score",
                )?;
                idx += 1;
            }
            "--scan-pages" => {
                idx += 1;
                cmd.scan_pages = parse_i64(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --scan-pages 的值".to_string()))?,
                    "--scan-pages",
                )?
                .max(1);
                idx += 1;
            }
            "--page-size" => {
                idx += 1;
                cmd.page_size = parse_i64(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --page-size 的值".to_string()))?,
                    "--page-size",
                )?
                .clamp(1, 200);
                idx += 1;
            }
            "--limit" => {
                idx += 1;
                cmd.limit = parse_usize(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --limit 的值".to_string()))?,
                    "--limit",
                )?;
                idx += 1;
            }
            "--status" => {
                idx += 1;
                cmd.status = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --status 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            "--alias" => {
                idx += 1;
                cmd.alias = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --alias 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            unknown => {
                return Err(CliError::Args(format!("suspicious 不支持参数: {unknown}")));
            }
        }
    }
    Ok(cmd)
}

fn parse_status_cmd(rest: &[String]) -> Result<UserHashCmd, CliError> {
    let user_hash = parse_user_hash_flag(rest, "status")?;
    Ok(UserHashCmd { user_hash })
}

fn parse_set_status_cmd(rest: &[String]) -> Result<SetStatusCmd, CliError> {
    let mut user_hash = None;
    let mut status = None;
    let mut reason = None;

    let mut idx = 0usize;
    while idx < rest.len() {
        match rest[idx].as_str() {
            "--user-hash" => {
                idx += 1;
                user_hash = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --user-hash 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            "--status" => {
                idx += 1;
                status = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --status 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            "--reason" => {
                idx += 1;
                reason = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --reason 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            unknown => {
                return Err(CliError::Args(format!("set-status 不支持参数: {unknown}")));
            }
        }
    }

    let user_hash = user_hash.ok_or_else(|| CliError::Args("缺少 --user-hash".to_string()))?;
    let status = status.ok_or_else(|| CliError::Args("缺少 --status".to_string()))?;

    Ok(SetStatusCmd {
        user_hash,
        status,
        reason,
    })
}

fn parse_user_hash_reason_cmd(
    rest: &[String],
    cmd_name: &str,
) -> Result<UserHashReasonCmd, CliError> {
    let mut user_hash = None;
    let mut reason = None;

    let mut idx = 0usize;
    while idx < rest.len() {
        match rest[idx].as_str() {
            "--user-hash" => {
                idx += 1;
                user_hash = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --user-hash 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            "--reason" => {
                idx += 1;
                reason = Some(
                    rest.get(idx)
                        .ok_or_else(|| CliError::Args("缺少 --reason 的值".to_string()))?
                        .to_string(),
                );
                idx += 1;
            }
            unknown => {
                return Err(CliError::Args(format!("{cmd_name} 不支持参数: {unknown}")));
            }
        }
    }

    let user_hash = user_hash.ok_or_else(|| CliError::Args("缺少 --user-hash".to_string()))?;
    Ok(UserHashReasonCmd { user_hash, reason })
}

fn parse_user_hash_flag(rest: &[String], cmd_name: &str) -> Result<String, CliError> {
    let mut idx = 0usize;
    while idx < rest.len() {
        if rest[idx] == "--user-hash" {
            idx += 1;
            return Ok(rest
                .get(idx)
                .ok_or_else(|| CliError::Args("缺少 --user-hash 的值".to_string()))?
                .to_string());
        }
        return Err(CliError::Args(format!(
            "{cmd_name} 不支持参数: {}",
            rest[idx]
        )));
    }
    Err(CliError::Args("缺少 --user-hash".to_string()))
}

fn parse_i64(raw: &str, flag: &str) -> Result<i64, CliError> {
    raw.parse::<i64>()
        .map_err(|_| CliError::Args(format!("{flag} 需要整数，收到: {raw}")))
}

fn parse_u64(raw: &str, flag: &str) -> Result<u64, CliError> {
    raw.parse::<u64>()
        .map_err(|_| CliError::Args(format!("{flag} 需要整数，收到: {raw}")))
}

fn parse_f64(raw: &str, flag: &str) -> Result<f64, CliError> {
    raw.parse::<f64>()
        .map_err(|_| CliError::Args(format!("{flag} 需要数字，收到: {raw}")))
}

fn parse_usize(raw: &str, flag: &str) -> Result<usize, CliError> {
    raw.parse::<usize>()
        .map_err(|_| CliError::Args(format!("{flag} 需要正整数，收到: {raw}")))
}

impl AdminApi {
    fn new(base_url: String, admin_token: String, timeout_secs: u64) -> Result<Self, CliError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| CliError::Config(format!("创建 HTTP Client 失败: {e}")))?;
        Ok(Self {
            client,
            base_url,
            admin_token,
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    async fn get_users(
        &self,
        query: &UsersQuery,
    ) -> Result<AdminLeaderboardUsersResponse, CliError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(v) = query.page {
            params.push(("page", v.to_string()));
        }
        if let Some(v) = query.page_size {
            params.push(("pageSize", v.to_string()));
        }
        if let Some(v) = query.status.as_ref() {
            params.push(("status", v.clone()));
        }
        if let Some(v) = query.alias.as_ref() {
            params.push(("alias", v.clone()));
        }

        let req = self
            .client
            .request(Method::GET, self.endpoint("/admin/leaderboard/users"))
            .header("X-Admin-Token", &self.admin_token)
            .query(&params);
        self.send_json(req).await
    }

    async fn get_user_status(&self, user_hash: &str) -> Result<AdminUserStatusResponse, CliError> {
        let req = self
            .client
            .request(Method::GET, self.endpoint("/admin/users/status"))
            .header("X-Admin-Token", &self.admin_token)
            .query(&[("userHash", user_hash)]);
        self.send_json(req).await
    }

    async fn set_user_status(
        &self,
        user_hash: &str,
        status: &str,
        reason: Option<String>,
    ) -> Result<AdminUserStatusResponse, CliError> {
        let body = AdminSetUserStatusRequest {
            user_hash: user_hash.to_string(),
            status: status.to_string(),
            reason,
        };
        let req = self
            .client
            .request(Method::POST, self.endpoint("/admin/users/status"))
            .header("X-Admin-Token", &self.admin_token)
            .json(&body);
        self.send_json(req).await
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, CliError> {
        let res = req
            .send()
            .await
            .map_err(|e| CliError::Network(format!("请求失败: {e}")))?;
        let status = res.status();
        let body = res
            .text()
            .await
            .map_err(|e| CliError::Network(format!("读取响应失败: {e}")))?;
        if status.is_success() {
            return serde_json::from_str::<T>(&body)
                .map_err(|e| CliError::Decode(format!("{e}; body={body}")));
        }

        if let Ok(problem) = serde_json::from_str::<ProblemDetails>(&body) {
            return Err(CliError::Api {
                status: problem.status,
                code: problem.code,
                detail: problem.detail.unwrap_or(problem.title),
                request_id: problem.request_id,
            });
        }

        Err(CliError::Api {
            status: status.as_u16(),
            code: "UNKNOWN".to_string(),
            detail: body,
            request_id: None,
        })
    }
}

fn load_runtime_defaults() -> RuntimeDefaults {
    if phi_backend::AppConfig::init_global().is_ok() {
        let cfg = phi_backend::AppConfig::global();
        let host = if cfg.server.host == "0.0.0.0" {
            "127.0.0.1".to_string()
        } else {
            cfg.server.host.clone()
        };
        let base_url = format!("http://{}:{}{}", host, cfg.server.port, cfg.api.prefix);
        let admin_token = cfg
            .leaderboard
            .admin_tokens
            .iter()
            .find(|t| !t.trim().is_empty())
            .cloned();
        return RuntimeDefaults {
            base_url,
            admin_token,
        };
    }
    RuntimeDefaults {
        base_url: DEFAULT_BASE_URL.to_string(),
        admin_token: None,
    }
}

fn resolve_admin_token(
    token_from_arg: Option<String>,
    token_env_name: &str,
    token_from_config: Option<String>,
) -> Result<String, CliError> {
    if let Some(v) = token_from_arg {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Ok(v);
        }
    }
    if let Ok(v) = env::var(token_env_name) {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Ok(v);
        }
    }
    if let Some(v) = token_from_config {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Ok(v);
        }
    }
    Err(CliError::Config(format!(
        "未找到管理员令牌。请使用 --token，或设置环境变量 {token_env_name}，或在 config.toml 配置 leaderboard.admin_tokens"
    )))
}

async fn run_users(api: &AdminApi, cmd: UsersCmd, as_json: bool) -> Result<(), CliError> {
    let resp = api
        .get_users(&UsersQuery {
            page: Some(cmd.page),
            page_size: Some(cmd.page_size.clamp(1, 200)),
            status: cmd.status.clone(),
            alias: cmd.alias.clone(),
        })
        .await?;

    if as_json {
        print_json(&resp)?;
        return Ok(());
    }

    println!(
        "total={} page={} pageSize={} returned={}",
        resp.total,
        resp.page,
        resp.page_size,
        resp.items.len()
    );
    print_user_items(&resp.items);
    Ok(())
}

async fn run_suspicious(api: &AdminApi, cmd: SuspiciousCmd, as_json: bool) -> Result<(), CliError> {
    let mut scanned_pages = 0_i64;
    let mut all: Vec<AdminLeaderboardUserItem> = Vec::new();

    for page in 1..=cmd.scan_pages.max(1) {
        let resp = api
            .get_users(&UsersQuery {
                page: Some(page),
                page_size: Some(cmd.page_size.clamp(1, 200)),
                status: cmd.status.clone(),
                alias: cmd.alias.clone(),
            })
            .await?;
        scanned_pages += 1;
        all.extend(
            resp.items
                .into_iter()
                .filter(|x| x.suspicion >= cmd.min_score),
        );

        let reached_end = page * cmd.page_size >= resp.total;
        if reached_end {
            break;
        }
    }

    all.sort_by(|a, b| {
        b.suspicion
            .partial_cmp(&a.suspicion)
            .unwrap_or(Ordering::Equal)
            .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal))
            .then_with(|| a.user_hash.cmp(&b.user_hash))
    });

    if all.len() > cmd.limit {
        all.truncate(cmd.limit);
    }

    if as_json {
        let payload = SuspiciousScanResult {
            min_score: cmd.min_score,
            scanned_pages,
            page_size: cmd.page_size,
            returned: all.len(),
            items: all,
        };
        print_json(&payload)?;
        return Ok(());
    }

    println!(
        "minScore={} scannedPages={} pageSize={} returned={}",
        cmd.min_score,
        scanned_pages,
        cmd.page_size,
        all.len()
    );
    print_user_items(&all);
    Ok(())
}

async fn run_status(api: &AdminApi, cmd: UserHashCmd, as_json: bool) -> Result<(), CliError> {
    let resp = api.get_user_status(&cmd.user_hash).await?;
    if as_json {
        print_json(&resp)?;
        return Ok(());
    }
    println!("userHash: {}", resp.user_hash);
    println!("status: {}", resp.status);
    println!("reason: {}", resp.reason.unwrap_or_else(|| "-".to_string()));
    println!(
        "updatedBy: {}",
        resp.updated_by.unwrap_or_else(|| "-".to_string())
    );
    println!(
        "updatedAt: {}",
        resp.updated_at.unwrap_or_else(|| "-".to_string())
    );
    Ok(())
}

async fn run_set_status(api: &AdminApi, cmd: SetStatusCmd, as_json: bool) -> Result<(), CliError> {
    let resp = api
        .set_user_status(&cmd.user_hash, &cmd.status, cmd.reason)
        .await?;
    if as_json {
        print_json(&resp)?;
        return Ok(());
    }
    println!("ok");
    println!("userHash: {}", resp.user_hash);
    println!("status: {}", resp.status);
    println!("reason: {}", resp.reason.unwrap_or_else(|| "-".to_string()));
    println!(
        "updatedBy: {}",
        resp.updated_by.unwrap_or_else(|| "-".to_string())
    );
    println!(
        "updatedAt: {}",
        resp.updated_at.unwrap_or_else(|| "-".to_string())
    );
    Ok(())
}

fn print_json<T: Serialize>(data: &T) -> Result<(), CliError> {
    let s = serde_json::to_string_pretty(data)
        .map_err(|e| CliError::Decode(format!("序列化 JSON 失败: {e}")))?;
    println!("{s}");
    Ok(())
}

fn print_user_items(items: &[AdminLeaderboardUserItem]) {
    println!("userHash\talias\tscore\tsuspicion\tstatus\thidden\tupdatedAt");
    for x in items {
        println!(
            "{}\t{}\t{:.4}\t{:.4}\t{}\t{}\t{}",
            x.user_hash,
            x.alias.as_deref().unwrap_or("-"),
            x.score,
            x.suspicion,
            x.status,
            x.is_hidden,
            x.updated_at
        );
    }
}

fn print_help() {
    println!(
        r#"admin_cli（管理员本地工具）

全局参数：
  --base-url URL            API 基地址（默认从 config 解析，否则 http://127.0.0.1:3939/api/v2）
  --token TOKEN             管理员令牌（Header: X-Admin-Token）
  --token-env NAME          从环境变量读取令牌（默认 PHI_ADMIN_TOKEN）
  --timeout-secs N          请求超时秒数（默认 15）
  --json                    JSON 输出（便于脚本集成）
  -h, --help                显示帮助

命令：
  users
    --page N                页码，默认 1
    --page-size N           每页条数，默认 50，范围 1-200
    --status S              active|approved|shadow|banned|rejected
    --alias KEYWORD         别名模糊筛选

  suspicious
    --min-score F           可疑分阈值，默认 0.6
    --scan-pages N          扫描前 N 页 users 数据，默认 5
    --page-size N           每页条数，默认 100，范围 1-200
    --limit N               最多返回条数，默认 200
    --status S              可选：仅扫描指定状态
    --alias KEYWORD         可选：按别名缩小范围

  status
    --user-hash HASH        查询用户全局状态

  set-status
    --user-hash HASH        目标用户完整 user_hash
    --status S              active|approved|shadow|banned|rejected
    --reason TEXT           可选备注

  ban
    --user-hash HASH        封禁用户（状态设为 banned）
    --reason TEXT           可选备注

  unban
    --user-hash HASH        解封用户（状态设为 active）
    --reason TEXT           可选备注

示例：
  cargo run --bin admin_cli -- users --page 1 --page-size 50
  cargo run --bin admin_cli -- suspicious --min-score 1.0 --scan-pages 10
  cargo run --bin admin_cli -- status --user-hash abcdef123456...
  cargo run --bin admin_cli -- ban --user-hash abcdef123456... --reason "manual review"
  cargo run --bin admin_cli -- unban --user-hash abcdef123456... --reason "appeal passed"
"#
    );
}

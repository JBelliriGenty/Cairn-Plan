//! Cairn Plan: projects and tickets for a small team, behind an
//! authenticating reverse proxy (e.g. Traefik + Authelia). The proxy signs
//! people in; Cairn Plan keeps no password and only trusts the identity
//! headers of the proxies listed in the configuration.
//! Rules and data: store.rs. Configuration: docs/configuration.md.

mod hooks;
mod mcp;
mod notify;
mod store;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use store::Actor;
use tiny_http::{Header, Method, Request, Response, Server};

const INDEX: &str = include_str!("../www/index.html");
const JS: &str = include_str!("../www/plan.js");
const CSS: &str = include_str!("../www/plan.css");
const I18N: &str = include_str!("../www/i18n.js");
const ICON: &str = include_str!("../www/icon.svg");
// Mermaid, pinned (11.12.0, MIT): served from here, never from a CDN, and only
// fetched by a page that shows a ```mermaid block. The version is in the name,
// so the file can be cached for good.
const MERMAID: &str = include_str!("../www/mermaid-11.12.0.min.js");
const MERMAID_PATH: &str = "mermaid-11.12.0.min.js";

/// Every key read from the configuration file (checked against the docs by a test).
const KEYS: [&str; 22] = [
    "listen", "trusted_proxies", "require_groups", "admin_groups", "db", "lang", "mcp_keys_file", "mcp_login", "mcp_accounts",
    "webhook_secrets_file",
    "public_url", "smtp_host", "smtp_port", "smtp_credentials_file", "mail_from", "ntfy_url", "ntfy_token_file", "ntfy_topics",
    "agent_hook_url", "agent_hook_secret_file", "oidc_issuer", "oidc_client_id",
];

struct Config {
    listen: String,
    trusted_proxies: Vec<String>,
    require_groups: Vec<String>,
    admin_groups: Vec<String>,
    db: String,
    lang: String,
    /// (name, key) pairs allowed on /mcp; empty: no MCP server.
    mcp_keys: Vec<(String, String)>,
    /// The account MCP calls made with a key run as, unless mcp_accounts names another for the key.
    mcp_login: String,
    /// (issuer, client id) of the OpenID provider whose access tokens are
    /// accepted on /mcp: such a call runs as the person signed in.
    oidc: Option<(String, String)>,
    /// Key name -> account: one account per agent role.
    mcp_accounts: HashMap<String, String>,
    /// (name, secret) pairs that may sign a forge webhook; empty: no webhook.
    webhook_secrets: Vec<(String, String)>,
    /// Where notifications go besides the page (notify.rs).
    outside: notify::Outside,
    /// (address, secret) of the webhook told about tickets to work on; empty address: none.
    agent_hook: (String, String),
    /// Folder of the files attached to tickets: "files", next to the database.
    files: std::path::PathBuf,
    /// Folder of the profile photos: "avatars", next to the database.
    avatars: std::path::PathBuf,
}

/// Largest file that can be attached to a ticket.
const MAX_FILE: u64 = 25 * 1024 * 1024;
/// Largest profile photo. The page sends a square of 256 pixels, far smaller.
const MAX_PHOTO: u64 = 1024 * 1024;

/// The "name:secret" lines of the file named by a configuration key (secret
/// of 32 characters or more, lines starting with # ignored). The name only
/// shows in the log.
fn secrets_file(v: &Value, key: &str) -> Vec<(String, String)> {
    let Some(f) = v[key].as_str() else { return Vec::new() };
    let raw = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("cannot read {key} {f}: {e}"));
    raw.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.trim().split_once(':'))
        .filter(|(n, k)| !n.is_empty() && k.len() >= 32)
        .map(|(n, k)| (n.to_string(), k.to_string()))
        .collect()
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
}

impl Config {
    fn load(path: &str) -> Config {
        let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        let v: Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{path} is not valid JSON: {e}"));
        for k in v.as_object().map(|o| o.keys().collect::<Vec<_>>()).unwrap_or_default() {
            if !KEYS.contains(&k.as_str()) {
                eprintln!("warning: unknown configuration key \"{k}\"");
            }
        }
        let s = |k: &str, d: &str| v[k].as_str().unwrap_or(d).to_string();
        // the one line of the file named by a configuration key; no key: empty
        let secret = |k: &str| {
            v[k].as_str().map(|f| std::fs::read_to_string(f).unwrap_or_else(|e| panic!("cannot read {k} {f}: {e}")).trim().to_string()).unwrap_or_default()
        };
        let cfg = Config {
            listen: s("listen", "127.0.0.1:9094"),
            trusted_proxies: strings(&v, "trusted_proxies"),
            require_groups: strings(&v, "require_groups"),
            admin_groups: strings(&v, "admin_groups"),
            db: s("db", "/var/lib/cairn-plan/plan.db"),
            lang: s("lang", ""),
            mcp_keys: secrets_file(&v, "mcp_keys_file"),
            mcp_login: s("mcp_login", "claude"),
            oidc: v["oidc_issuer"].as_str().map(|i| (i.trim_end_matches('/').to_string(), s("oidc_client_id", ""))),
            mcp_accounts: v["mcp_accounts"]
                .as_object()
                .map(|o| o.iter().filter_map(|(k, l)| l.as_str().map(|l| (k.clone(), l.to_string()))).collect())
                .unwrap_or_default(),
            webhook_secrets: secrets_file(&v, "webhook_secrets_file"),
            files: std::path::Path::new(&s("db", "/var/lib/cairn-plan/plan.db")).with_file_name("files"),
            avatars: std::path::Path::new(&s("db", "/var/lib/cairn-plan/plan.db")).with_file_name("avatars"),
            agent_hook: (s("agent_hook_url", ""), secret("agent_hook_secret_file")),
            outside: notify::Outside {
                public_url: s("public_url", "").trim_end_matches('/').to_string(),
                lang: s("lang", ""),
                smtp: v["smtp_host"].as_str().map(|host| {
                    // "user:password" on one line
                    let credentials = secret("smtp_credentials_file");
                    let (user, password) = credentials.split_once(':').unwrap_or(("", ""));
                    notify::Smtp {
                        host: host.to_string(),
                        port: v["smtp_port"].as_u64().unwrap_or(587) as u16,
                        user: user.to_string(),
                        password: password.to_string(),
                        from: s("mail_from", "Cairn Plan <cairn-plan@localhost>"),
                    }
                }),
                ntfy: v["ntfy_url"].as_str().map(|url| notify::Ntfy {
                    url: url.to_string(),
                    token: secret("ntfy_token_file"),
                    topics: v["ntfy_topics"]
                        .as_object()
                        .map(|o| o.iter().filter_map(|(login, t)| t.as_str().map(|t| (login.clone(), t.to_string()))).collect())
                        .unwrap_or_default(),
                }),
            },
        };
        assert!(store::valid_login(&cfg.mcp_login), "mcp_login is not a valid login");
        assert!(cfg.mcp_accounts.values().all(|l| store::valid_login(l)), "mcp_accounts holds an invalid login");
        assert!(!cfg.trusted_proxies.is_empty(), "trusted_proxies is empty: nobody could sign in");
        if let Some((_, client)) = &cfg.oidc {
            assert!(!client.is_empty(), "oidc_issuer is set without oidc_client_id");
            assert!(!cfg.outside.public_url.is_empty(), "oidc_issuer is set without public_url");
        }
        cfg
    }
}

static DB: OnceLock<Mutex<rusqlite::Connection>> = OnceLock::new();

fn header(r: &Request, name: &str) -> String {
    r.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().trim().to_string()).unwrap_or_default()
}

/// Who is asking, as told by a trusted proxy. None: not allowed in.
fn actor(cfg: &Config, r: &Request) -> Option<(Actor, String, String)> {
    let ip = r.remote_addr()?.ip().to_string();
    if !cfg.trusted_proxies.contains(&ip) {
        return None;
    }
    let login = header(r, "Remote-User").to_lowercase();
    if !store::valid_login(&login) {
        return None;
    }
    let groups = header(r, "Remote-Groups");
    let in_any = |wanted: &[String]| groups.split(',').any(|g| wanted.iter().any(|w| w == g.trim()));
    if !cfg.require_groups.is_empty() && !in_any(&cfg.require_groups) {
        return None;
    }
    let admin = in_any(&cfg.admin_groups);
    Some((Actor { login, admin }, header(r, "Remote-Name"), header(r, "Remote-Email")))
}

/// Compares two secrets without stopping at the first difference.
fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Address of the metadata that tells an MCP client where to sign in (RFC 9728).
fn resource_metadata_url(cfg: &Config) -> String {
    format!("{}/.well-known/oauth-protected-resource/mcp", cfg.outside.public_url)
}

/// Protected resource metadata (RFC 9728): /mcp, and the provider that issues its tokens.
fn resource_metadata(cfg: &Config) -> Option<Value> {
    let (issuer, _) = cfg.oidc.as_ref()?;
    Some(json!({
        "resource": format!("{}/mcp", cfg.outside.public_url),
        "authorization_servers": [issuer],
        "scopes_supported": ["openid", "profile", "email", "groups", "offline_access"],
        "bearer_methods_supported": ["header"],
    }))
}

/// The claims of a JWT, read without checking its signature: used only once
/// the provider has accepted that very token. Null when it is not a JWT.
fn jwt_claims(token: &str) -> Value {
    let Some(part) = token.split('.').nth(1) else { return Value::Null };
    let (mut bits, mut n, mut out) = (0u32, 0, Vec::new());
    for c in part.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => break,
        };
        bits = bits << 6 | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    serde_json::from_slice(&out).unwrap_or(Value::Null)
}

/// Who an access token belongs to, from what the provider's userinfo
/// endpoint answered: (actor, name, e-mail), or why it is refused. The token
/// must be meant for this server: "aud" holds oidc_client_id or the address
/// of /mcp (the resource the client asked for, RFC 8707), or "azp" /
/// "client_id" is oidc_client_id. A token another application received for
/// the same person is not enough.
fn token_owner(cfg: &Config, info: &Value) -> Result<(Actor, String, String), String> {
    let (_, client) = cfg.oidc.as_ref().ok_or("no OpenID provider")?;
    let resource = format!("{}/mcp", cfg.outside.public_url);
    let ours = |v: &Value| v == client.as_str() || v == resource.as_str();
    let aud = &info["aud"];
    let for_us = ours(aud) || aud.as_array().is_some_and(|a| a.iter().any(ours)) || info["azp"] == client.as_str() || info["client_id"] == client.as_str();
    if !for_us {
        return Err(format!("token not issued to {client} (aud {aud}, azp {}, client_id {})", info["azp"], info["client_id"]));
    }
    let login = info["preferred_username"].as_str().unwrap_or("").to_lowercase();
    if !store::valid_login(&login) {
        return Err("no valid preferred_username (scope profile)".into());
    }
    let groups: Vec<&str> = info["groups"].as_array().map(|g| g.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let in_any = |wanted: &[String]| groups.iter().any(|g| wanted.iter().any(|w| w == g));
    if !cfg.require_groups.is_empty() && !in_any(&cfg.require_groups) {
        return Err(format!("{login} is in none of require_groups"));
    }
    let admin = in_any(&cfg.admin_groups);
    let text = |k: &str| info[k].as_str().unwrap_or("").to_string();
    Ok((Actor { login, admin }, text("name"), text("email")))
}

/// Asks the provider who holds this access token (its userinfo endpoint,
/// found once in its discovery document). The provider checks that the
/// token is genuine, unexpired and not revoked.
// ponytail: one call to the provider per MCP request; cache by token if it ever shows in latency
fn userinfo(issuer: &str, token: &str) -> Result<Value, String> {
    static ENDPOINT: OnceLock<String> = OnceLock::new();
    let get = |url: &str| ureq::get(url).timeout(std::time::Duration::from_secs(10));
    let endpoint = match ENDPOINT.get() {
        Some(e) => e.clone(),
        None => {
            let raw = get(&format!("{issuer}/.well-known/openid-configuration")).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
            let found = serde_json::from_str::<Value>(&raw).ok().and_then(|d| d["userinfo_endpoint"].as_str().map(String::from));
            ENDPOINT.get_or_init(|| found.unwrap_or_default()).clone()
        }
    };
    if endpoint.is_empty() {
        return Err("the provider announces no userinfo_endpoint".into());
    }
    let raw = get(&endpoint).set("Authorization", &format!("Bearer {token}")).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

/// MCP endpoint (mcp.rs), no proxy identity: "Authorization: Bearer" holds
/// either a key of mcp_keys_file (the call runs as the account mcp_accounts
/// gives the key, mcp_login by default; the key names the client in the log) or an access token from the OpenID provider (the
/// call runs as the person signed in, with their rights).
fn serve_mcp(cfg: &Config, mut r: Request) {
    if cfg.mcp_keys.is_empty() && cfg.oidc.is_none() {
        return respond(r, 404, "not found".into(), "text/plain");
    }
    let sent = header(&r, "Authorization");
    let key = sent.strip_prefix("Bearer ").unwrap_or("").trim();
    let caller = match cfg.mcp_keys.iter().find(|(_, k)| ct_eq(k, key)) {
        Some((client, _)) => {
            let login = cfg.mcp_accounts.get(client).unwrap_or(&cfg.mcp_login).clone();
            Some((client.clone(), Actor { login, admin: false }, String::new(), String::new()))
        }
        None => match &cfg.oidc {
            Some((issuer, _)) if !key.is_empty() => match userinfo(issuer, key).and_then(|mut info| {
                // Authelia's userinfo carries no audience: take it from the token itself
                let claims = jwt_claims(key);
                for k in ["aud", "azp", "client_id"] {
                    if info[k].is_null() && info.is_object() {
                        info[k] = claims[k].clone();
                    }
                }
                token_owner(cfg, &info)
            }) {
                Ok((who, name, email)) => Some((format!("oauth {}", who.login), who, name, email)),
                Err(e) => {
                    eprintln!("mcp: access token refused: {e}");
                    None
                }
            },
            _ => None,
        },
    };
    let Some((client, who, name, email)) = caller else {
        let challenge = match cfg.oidc {
            Some(_) => format!("Bearer resource_metadata=\"{}\"", resource_metadata_url(cfg)),
            None => "Bearer".to_string(),
        };
        let _ = r.respond(
            Response::from_string("unauthorized").with_status_code(401).with_header(Header::from_bytes("WWW-Authenticate", challenge.as_bytes()).unwrap()),
        );
        return;
    };
    if r.method() != &Method::Post {
        // no server-to-client stream: answers come in the response of each POST
        return respond(r, 405, "method not allowed".into(), "text/plain");
    }
    let mut raw = String::new();
    let _ = r.as_reader().take(200_000).read_to_string(&mut raw);
    let Ok(req) = serde_json::from_str::<Value>(&raw) else {
        let e = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}});
        return respond(r, 400, e.to_string(), "application/json");
    };
    if req["method"] == "tools/call" {
        let a = &req["params"]["arguments"];
        let target = a["id"].as_str().or(a["project"].as_str()).unwrap_or("");
        eprintln!("mcp {client}: {} {target}", req["params"]["name"].as_str().unwrap_or("?"));
    }
    let c = DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
    store::seen(&c, &who.login, &name, &email);
    let out = mcp::handle(&c, &who, &req);
    drop(c);
    match out {
        Some(v) => respond(r, 200, v.to_string(), "application/json"),
        None => respond(r, 202, String::new(), "application/json"),
    }
}

/// Forge webhooks (hooks.rs): no proxy identity, the body is signed with a
/// shared secret. The secret says which forge calls; it can only add links.
fn serve_hook(cfg: &Config, mut r: Request) {
    if cfg.webhook_secrets.is_empty() {
        return respond(r, 404, "not found".into(), "text/plain");
    }
    if r.method() != &Method::Post {
        return respond(r, 405, "method not allowed".into(), "text/plain");
    }
    // a body cut at the limit fails the signature check
    let mut raw = Vec::new();
    let _ = r.as_reader().take(2_000_000).read_to_end(&mut raw);
    let signature = header(&r, "X-Hub-Signature-256");
    let Some((forge, _)) = cfg.webhook_secrets.iter().find(|(_, s)| hooks::signed(s, &raw, &signature)) else {
        return respond(r, 401, "bad signature".into(), "text/plain");
    };
    let Ok(payload) = serde_json::from_slice::<Value>(&raw) else {
        return respond(r, 400, "invalid request".into(), "text/plain");
    };
    let c = DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
    let linked = hooks::handle(&c, &payload);
    drop(c);
    let repo = payload["repository"]["full_name"].as_str().unwrap_or("?");
    eprintln!("hook {forge}: {} {repo} -> {}", header(&r, "X-GitHub-Event"), linked.join(" "));
    respond(r, 200, json!({ "linked": linked }).to_string(), "application/json")
}

/// The accounts MCP calls run as: those of the agents.
fn agent_logins(cfg: &Config) -> Vec<String> {
    std::iter::once(cfg.mcp_login.clone()).chain(cfg.mcp_accounts.values().cloned()).collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        let byte = if b[i] == b'%' {
            b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok())
        } else {
            None
        };
        match byte {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// How a file is handed back, by its extension: (content type, disposition).
/// Shown by the browser for the few kinds that are safe to show, downloaded
/// otherwise: an attached HTML or SVG file must never run as a page of this site.
fn kind(name: &str) -> (&'static str, &'static str) {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => ("image/png", "inline"),
        "jpg" | "jpeg" => ("image/jpeg", "inline"),
        "gif" => ("image/gif", "inline"),
        "webp" => ("image/webp", "inline"),
        "pdf" => ("application/pdf", "inline"),
        _ => ("application/octet-stream", "attachment"),
    }
}

/// Files attached to a ticket, the one part of the API that is not JSON:
/// POST api/tickets/<id>/files?name=<name> with the bytes adds one,
/// GET api/tickets/<id>/files/<n> reads it; POST with {"delete": <n>} removes it.
fn serve_file(cfg: &Config, mut r: Request, who: &Actor, id: &str, rest: &[&str]) {
    let fail = |r: Request, code: u16, e: &str| respond(r, code, json!({ "error": e }).to_string(), "application/json");
    let db = || DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
    let method = r.method().clone();
    match (&method, rest) {
        (Method::Post, []) if header(&r, "Content-Type").starts_with("application/json") => {
            let mut raw = String::new();
            let _ = r.as_reader().take(1000).read_to_string(&mut raw);
            let body: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            let done = store::delete_file(&db(), who, id, &body);
            match done {
                Ok(fid) => {
                    let _ = std::fs::remove_file(cfg.files.join(fid.to_string()));
                    respond(r, 200, json!({ "ok": true }).to_string(), "application/json")
                }
                Err((code, e)) => fail(r, code, &e),
            }
        }
        (Method::Post, []) => {
            // CSRF: a page of another site cannot send this content type without a
            // CORS preflight, which this server never answers.
            if !header(&r, "Content-Type").starts_with("application/octet-stream") {
                return fail(r, 400, "invalid request");
            }
            if r.body_length().is_some_and(|n| n as u64 > MAX_FILE) {
                return fail(r, 413, "file too large");
            }
            let name = r.url().split_once('?').map(|(_, q)| q).unwrap_or("").split('&').find_map(|p| p.strip_prefix("name=")).map(percent_decode).unwrap_or_default();
            let mut data = Vec::new();
            let _ = r.as_reader().take(MAX_FILE + 1).read_to_end(&mut data);
            if data.is_empty() {
                return fail(r, 400, "empty file");
            }
            if data.len() as u64 > MAX_FILE {
                return fail(r, 413, "file too large");
            }
            let c = db();
            let saved = store::add_file(&c, who, id, &name, data.len() as i64).and_then(|fid| {
                std::fs::write(cfg.files.join(fid.to_string()), &data).map(|_| fid).map_err(|e| {
                    eprintln!("file {fid} of {id} not saved: {e}");
                    let _ = store::delete_file(&c, who, id, &json!({ "delete": fid }));
                    (500, "the file could not be saved".to_string())
                })
            });
            drop(c);
            match saved {
                Ok(fid) => respond(r, 200, json!({ "id": fid }).to_string(), "application/json"),
                Err((code, e)) => fail(r, code, &e),
            }
        }
        (Method::Get, [n]) => {
            let found = n.parse::<i64>().map_err(|_| (404, "no such file".to_string())).and_then(|fid| store::file(&db(), who, id, fid).map(|name| (fid, name)));
            let (fid, name) = match found {
                Ok(x) => x,
                Err((code, e)) => return fail(r, code, &e),
            };
            let Ok(data) = std::fs::read(cfg.files.join(fid.to_string())) else {
                return fail(r, 404, "no such file");
            };
            let (ctype, how) = kind(&name);
            let h = |k: &str, v: &str| Header::from_bytes(k, v).unwrap();
            let _ = r.respond(
                Response::from_data(data)
                    .with_header(h("Content-Type", ctype))
                    .with_header(h("Content-Disposition", &format!("{how}; filename*=UTF-8''{}", percent_encode(&name))))
                    .with_header(h("Cache-Control", "private, no-store"))
                    .with_header(h("X-Content-Type-Options", "nosniff"))
                    .with_header(h("Referrer-Policy", "no-referrer")),
            );
        }
        _ => fail(r, 404, "not found"),
    }
}

/// The content type of a profile photo, read from its first bytes: PNG, JPEG
/// or WebP. None: not one of them (an SVG or an HTML file is never kept).
fn image_kind(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Profile photos, not JSON either: POST api/avatars/<login> with the bytes
/// of an image sets the photo of an account, POST with {"delete": true}
/// removes it, GET reads it. Anyone signed in sees the photos; store::set_photo
/// says who may set one. One file per account, "<login>.img".
fn serve_avatar(cfg: &Config, mut r: Request, who: &Actor, login: &str) {
    let fail = |r: Request, code: u16, e: &str| respond(r, code, json!({ "error": e }).to_string(), "application/json");
    let db = || DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
    let file = cfg.avatars.join(format!("{login}.img"));
    let method = r.method().clone();
    let ctype = header(&r, "Content-Type");
    match method {
        Method::Get => {
            let found = if store::valid_login(login) { std::fs::read(&file).ok() } else { None };
            let Some((data, kind)) = found.and_then(|data| image_kind(&data).map(|kind| (data, kind))) else {
                return fail(r, 404, "no such photo");
            };
            let h = |k: &str, v: &str| Header::from_bytes(k, v).unwrap();
            // the address carries the version of the photo (?v=): kept by the browser for good
            let _ = r.respond(
                Response::from_data(data)
                    .with_header(h("Content-Type", kind))
                    .with_header(h("Cache-Control", "private, max-age=31536000, immutable"))
                    .with_header(h("X-Content-Type-Options", "nosniff"))
                    .with_header(h("Referrer-Policy", "no-referrer")),
            );
        }
        Method::Post if ctype.starts_with("application/json") => {
            let mut raw = String::new();
            let _ = r.as_reader().take(1000).read_to_string(&mut raw);
            if serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null)["delete"] != true {
                return fail(r, 400, "invalid request");
            }
            let done = store::set_photo(&db(), who, login, &agent_logins(cfg), false);
            match done {
                Ok(_) => {
                    let _ = std::fs::remove_file(&file);
                    if login != who.login {
                        eprintln!("photo of {login} removed by {}", who.login);
                    }
                    respond(r, 200, json!({ "photo": 0 }).to_string(), "application/json")
                }
                Err((code, e)) => fail(r, code, &e),
            }
        }
        // CSRF: same rule as the attached files
        Method::Post if ctype.starts_with("application/octet-stream") => {
            if r.body_length().is_some_and(|n| n as u64 > MAX_PHOTO) {
                return fail(r, 413, "image too large");
            }
            let mut data = Vec::new();
            let _ = r.as_reader().take(MAX_PHOTO + 1).read_to_end(&mut data);
            if data.len() as u64 > MAX_PHOTO {
                return fail(r, 413, "image too large");
            }
            if image_kind(&data).is_none() {
                return fail(r, 400, "not an image (PNG, JPEG or WebP)");
            }
            let c = db();
            let agents = agent_logins(cfg);
            let saved = store::set_photo(&c, who, login, &agents, true).and_then(|version| {
                std::fs::write(&file, &data).map(|_| version).map_err(|e| {
                    eprintln!("photo of {login} not saved: {e}");
                    let _ = store::set_photo(&c, who, login, &agents, false);
                    (500, "the file could not be saved".to_string())
                })
            });
            drop(c);
            match saved {
                Ok(version) => respond(r, 200, json!({ "photo": version }).to_string(), "application/json"),
                Err((code, e)) => fail(r, code, &e),
            }
        }
        _ => fail(r, 400, "invalid request"),
    }
}

/// Where notices go right now: what an administrator saved on the settings
/// page, and for what was not saved there, the configuration file.
fn outside(cfg: &Config, saved: &HashMap<String, String>) -> notify::Outside {
    let get = |key: &str, file: &str| saved.get(key).cloned().unwrap_or_else(|| file.to_string());
    let (smtp, ntfy) = (cfg.outside.smtp.as_ref(), cfg.outside.ntfy.as_ref());
    let host = get("smtp_host", smtp.map(|s| s.host.as_str()).unwrap_or(""));
    let url = get("ntfy_url", ntfy.map(|n| n.url.as_str()).unwrap_or(""));
    notify::Outside {
        public_url: get("public_url", &cfg.outside.public_url).trim_end_matches('/').to_string(),
        lang: cfg.lang.clone(),
        smtp: (!host.is_empty()).then(|| notify::Smtp {
            host,
            port: get("smtp_port", &smtp.map(|s| s.port).unwrap_or(587).to_string()).parse().unwrap_or(587),
            user: get("smtp_user", smtp.map(|s| s.user.as_str()).unwrap_or("")),
            password: get("smtp_password", smtp.map(|s| s.password.as_str()).unwrap_or("")),
            from: get("mail_from", smtp.map(|s| s.from.as_str()).unwrap_or("Cairn Plan <cairn-plan@localhost>")),
        }),
        ntfy: (!url.is_empty()).then(|| notify::Ntfy {
            url,
            token: get("ntfy_token", ntfy.map(|n| n.token.as_str()).unwrap_or("")),
            topics: ntfy.map(|n| n.topics.clone()).unwrap_or_default(),
        }),
    }
}

/// (address, secret) of the agent webhook in force: the settings page, else
/// the configuration file. An empty address: no webhook.
fn agent_hook(cfg: &Config, saved: &HashMap<String, String>) -> (String, String) {
    let get = |key: &str, file: &str| saved.get(key).cloned().unwrap_or_else(|| file.to_string());
    (get("agent_hook_url", &cfg.agent_hook.0), get("agent_hook_secret", &cfg.agent_hook.1))
}

/// The settings as the page shows them: what is in force, without the secrets.
fn settings_view(cfg: &Config, saved: &HashMap<String, String>) -> Value {
    let out = outside(cfg, saved);
    let (smtp, ntfy) = (out.smtp.as_ref(), out.ntfy.as_ref());
    let (hook_url, hook_secret) = agent_hook(cfg, saved);
    json!({
        "agent_hook_url": hook_url,
        "agent_hook_secret_set": !hook_secret.is_empty(),
        "public_url": out.public_url,
        "smtp_host": smtp.map(|s| s.host.as_str()).unwrap_or(""),
        "smtp_port": smtp.map(|s| s.port).unwrap_or(587),
        "smtp_user": smtp.map(|s| s.user.as_str()).unwrap_or(""),
        "smtp_password_set": smtp.is_some_and(|s| !s.password.is_empty()),
        "mail_from": smtp.map(|s| s.from.as_str()).unwrap_or(""),
        "ntfy_url": ntfy.map(|n| n.url.as_str()).unwrap_or(""),
        "ntfy_token_set": ntfy.is_some_and(|n| !n.token.is_empty()),
        "ntfy_prefix": saved.get("ntfy_prefix").map(String::as_str).unwrap_or(""),
    })
}

/// {"kind": "mail" | "ntfy" | "hook"}: sends a test message to the
/// administrator who asks (for "hook", a "ping" event to the agent webhook),
/// with the settings in force, and answers what the server said.
fn test_send(cfg: &Config, r: Request, who: &Actor, email: &str, body: &Value) {
    let fail = |r: Request, code: u16, e: &str| respond(r, code, json!({ "error": e }).to_string(), "application/json");
    if !who.admin {
        return fail(r, 403, "only an administrator can change the settings");
    }
    // the lock is released before anything goes on the network
    let (saved, topic) = {
        let c = DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
        (store::settings(&c), store::ntfy_topic(&c, &who.login))
    };
    let sent = if body["kind"] == "hook" {
        let (url, secret) = agent_hook(cfg, &saved);
        let ping = store::Call { event: "ping".into(), ticket: String::new(), project: String::new(), title: String::new(), actor: who.login.clone(), at: 0 };
        if url.is_empty() { Err("no agent webhook is set".to_string()) } else { hooks::send(&url, &secret, &ping) }
    } else {
        notify::test(&outside(cfg, &saved), body["kind"].as_str().unwrap_or(""), email, &topic)
    };
    match sent {
        Ok(()) => respond(r, 200, json!({ "ok": true }).to_string(), "application/json"),
        Err(e) => fail(r, 400, &e),
    }
}

// The page alone gets `style-src 'unsafe-inline'`: Mermaid puts a <style> element and
// style attributes in each SVG it draws. `script-src` stays closed (`default-src 'self'`).
fn csp(ctype: &str) -> &'static str {
    if ctype == "text/html" {
        "default-src 'self'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'"
    } else {
        "default-src 'self'; frame-ancestors 'none'"
    }
}

fn respond(r: Request, code: u16, body: String, ctype: &str) {
    respond_cached(r, code, body, ctype, "no-store");
}

fn respond_cached(r: Request, code: u16, body: String, ctype: &str, cache: &str) {
    let h = |k: &str, v: &str| Header::from_bytes(k, v).unwrap();
    let _ = r.respond(
        Response::from_string(body)
            .with_status_code(code)
            .with_header(h("Content-Type", &format!("{ctype}; charset=utf-8")))
            .with_header(h("Cache-Control", cache))
            .with_header(h("X-Content-Type-Options", "nosniff"))
            .with_header(h("Referrer-Policy", "no-referrer"))
            .with_header(h("Content-Security-Policy", csp(ctype))),
    );
}

fn handle(cfg: &'static Config, mut r: Request) {
    let path = r.url().split('?').next().unwrap_or("").to_string();
    if path == "/healthz" {
        return respond(r, 200, "ok".into(), "text/plain");
    }
    if path == "/mcp" {
        return serve_mcp(cfg, r);
    }
    if path == "/.well-known/oauth-protected-resource" || path == "/.well-known/oauth-protected-resource/mcp" {
        return match resource_metadata(cfg) {
            Some(m) => respond(r, 200, m.to_string(), "application/json"),
            None => respond(r, 404, "not found".into(), "text/plain"),
        };
    }
    if path == "/hooks/git" {
        return serve_hook(cfg, r);
    }
    let Some((who, name, email)) = actor(cfg, &r) else {
        return respond(r, 403, "access denied".into(), "text/plain");
    };
    let method = r.method().clone();
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    if let ["api", "tickets", id, "files", rest @ ..] = segs.as_slice() {
        return serve_file(cfg, r, &who, id, rest);
    }
    if let ["api", "avatars", login] = segs.as_slice() {
        return serve_avatar(cfg, r, &who, login);
    }
    let mut body = Value::Null;
    if method == Method::Post {
        // CSRF: a cross-site form cannot send a JSON body without a CORS
        // preflight, which this server never answers.
        if !header(&r, "Content-Type").starts_with("application/json") {
            return respond(r, 400, json!({"error": "invalid request"}).to_string(), "application/json");
        }
        let mut raw = String::new();
        let _ = r.as_reader().take(100_000).read_to_string(&mut raw);
        match serde_json::from_str(&raw) {
            Ok(v) => body = v,
            Err(_) => return respond(r, 400, json!({"error": "invalid request"}).to_string(), "application/json"),
        }
    }
    let page = |r: Request, text: &str, ctype: &str| respond(r, 200, text.into(), ctype);
    match (&method, segs.as_slice()) {
        (Method::Get, [""]) => return page(r, INDEX, "text/html"),
        (Method::Get, ["plan.js"]) => return page(r, JS, "text/javascript"),
        (Method::Get, ["plan.css"]) => return page(r, CSS, "text/css"),
        (Method::Get, ["i18n.js"]) => return page(r, I18N, "text/javascript"),
        (Method::Get, ["icon.svg"]) => return page(r, ICON, "image/svg+xml"),
        (Method::Get, [f]) if *f == MERMAID_PATH => {
            return respond_cached(r, 200, MERMAID.into(), "text/javascript", "private, max-age=31536000, immutable")
        }
        _ => {}
    }
    if let (Method::Post, ["api", "settings", "test"]) = (&method, segs.as_slice()) {
        return test_send(cfg, r, &who, &email, &body);
    }
    // ponytail: one connection behind one lock; a pool if requests ever queue.
    let c = DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
    let out = match (&method, segs.as_slice()) {
        (Method::Get, ["api", "me"]) => {
            store::seen(&c, &who.login, &name, &email);
            let saved = store::settings(&c);
            Ok(json!({
                "login": who.login, "name": name, "admin": who.admin, "lang": cfg.lang,
                "email": email, "mail": store::wants_mail(&c, &who.login), "max_file_mb": MAX_FILE / 1_048_576,
                "ntfy": outside(cfg, &saved).ntfy.is_some(), "ntfy_topic": store::ntfy_topic(&c, &who.login),
                "ntfy_prefix": saved.get("ntfy_prefix").map(String::as_str).unwrap_or(""),
                "version": env!("CARGO_PKG_VERSION"), "states": store::STATES, "priorities": store::PRIORITIES,
                "agents": agent_logins(cfg), "photos": store::photos(&c).unwrap_or_default(),
            }))
        }
        (Method::Post, ["api", "me"]) => store::set_prefs(&c, &who, &body),
        (Method::Get, ["api", "settings"]) if who.admin => Ok(settings_view(cfg, &store::settings(&c))),
        (Method::Get, ["api", "settings"]) => Err((403, "only an administrator can change the settings".to_string())),
        (Method::Post, ["api", "settings"]) => store::save_settings(&c, &who, &body),
        (Method::Get, ["api", "notifications"]) => store::notifications(&c, &who),
        (Method::Post, ["api", "notifications"]) => store::read_notifications(&c, &who, &body),
        (Method::Get, ["api", "mine"]) => store::mine(&c, &who),
        (Method::Get, ["api", "projects"]) => store::projects(&c, &who),
        (Method::Post, ["api", "projects"]) => store::create_project(&c, &who, &body),
        (Method::Get, ["api", "projects", key]) => store::project(&c, &who, key),
        (Method::Post, ["api", "projects", key]) => store::update_project(&c, &who, key, &body),
        (Method::Post, ["api", "projects", key, "members"]) => store::set_member(&c, &who, key, &body),
        (Method::Post, ["api", "projects", key, "tickets"]) => store::create_ticket(&c, &who, key, &body),
        (Method::Get, ["api", "tickets", id]) => store::ticket_view(&c, &who, id),
        (Method::Post, ["api", "tickets", id]) => store::update_ticket(&c, &who, id, &body),
        (Method::Post, ["api", "tickets", id, "comments"]) => store::comment(&c, &who, id, &body),
        (Method::Post, ["api", "tickets", id, "links"]) => store::link(&c, &who, id, &body),
        (Method::Post, ["api", "tickets", id, "work"]) => store::log_work(&c, &who, id, &body),
        (Method::Post, ["api", "rates"]) => store::set_rate(&c, &who, &body),
        (Method::Get, ["api", "projects", key, "time"]) => store::time_report(&c, &who, store::Scope::Project(key), &agent_logins(cfg)),
        (Method::Get, ["api", "spaces", name, "time"]) => {
            store::time_report(&c, &who, store::Scope::Space(&percent_decode(name)), &agent_logins(cfg))
        }
        _ => Err((404, "not found".to_string())),
    };
    drop(c);
    match out {
        Ok(v) => respond(r, 200, v.to_string(), "application/json"),
        Err((code, e)) => respond(r, code, json!({ "error": e }).to_string(), "application/json"),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: cairn-plan /etc/cairn-plan/config.json");
    if path == "member" {
        // cairn-plan member <config.json> <KEY> <login>: an agent account joins a project, as a
        // contributor and nothing more (an owner could approve its own work)
        let args: Vec<String> = std::env::args().skip(2).collect();
        let [config, key, login] = args.as_slice() else {
            panic!("usage: cairn-plan member <config.json> <KEY> <login>");
        };
        let root = Actor { login: "cairn-plan".into(), admin: true };
        match store::set_member(&store::open(&Config::load(config).db), &root, key, &json!({"login": login, "role": "contributor"})) {
            Ok(_) => println!("{login}: contributor of {key}"),
            Err((_, e)) => {
                eprintln!("member failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if path == "import" {
        // cairn-plan import <config.json> <draft.json> <space> <login>
        let args: Vec<String> = std::env::args().skip(2).collect();
        let [config, draft, space, login] = args.as_slice() else {
            panic!("usage: cairn-plan import <config.json> <draft.json> <space> <login>");
        };
        let raw = std::fs::read_to_string(draft).unwrap_or_else(|e| panic!("cannot read {draft}: {e}"));
        let draft: Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("the draft is not valid JSON: {e}"));
        match store::import(&store::open(&Config::load(config).db), login, space, &draft) {
            Ok(summary) => println!("{summary}"),
            Err((_, e)) => {
                eprintln!("import failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let cfg: &'static Config = Box::leak(Box::new(Config::load(&path)));
    DB.set(Mutex::new(store::open(&cfg.db))).ok().expect("database opened twice");
    std::fs::create_dir_all(&cfg.files).expect("cannot create the folder of attached files");
    std::fs::create_dir_all(&cfg.avatars).expect("cannot create the folder of profile photos");
    // notices waiting to leave the page: e-mail, ntfy (notify.rs)
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(10));
        // the lock is released before anything goes on the network
        let (waiting, calls, saved) = {
            let c = DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
            (store::outbox(&c), store::hook_outbox(&c), store::settings(&c))
        };
        notify::send(&outside(cfg, &saved), &waiting);
        // tickets an agent may work on (hooks.rs); a failure is written here
        // and not tried again: the ticket stays "todo" for whoever comes next
        let (url, secret) = agent_hook(cfg, &saved);
        for call in calls.iter().filter(|_| !url.is_empty()) {
            match hooks::send(&url, &secret, call) {
                Ok(()) => eprintln!("hook out: {} {}", call.event, call.ticket),
                Err(e) => eprintln!("hook out: {} {} not sent: {e}", call.event, call.ticket),
            }
        }
    });
    let server = Server::http(&cfg.listen).expect("cannot listen");
    eprintln!("cairn-plan {} listening on {}", env!("CARGO_PKG_VERSION"), cfg.listen);
    for r in server.incoming_requests() {
        std::thread::spawn(move || handle(cfg, r));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_names_travel_in_addresses_and_headers() {
        assert_eq!(super::percent_decode("Devis%20%C3%A9t%C3%A9.pdf"), "Devis été.pdf");
        assert_eq!(super::percent_decode("100%.txt%2"), "100%.txt%2");
        assert_eq!(super::percent_encode("Devis été.pdf"), "Devis%20%C3%A9t%C3%A9.pdf");
        assert_eq!(super::kind("a.PNG"), ("image/png", "inline"));
        assert_eq!(super::kind("page.html"), ("application/octet-stream", "attachment"));
        assert_eq!(super::kind("drawing.svg"), ("application/octet-stream", "attachment"));
    }

    #[test]
    fn a_profile_photo_is_a_png_a_jpeg_or_a_webp() {
        assert_eq!(super::image_kind(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Some("image/png"));
        assert_eq!(super::image_kind(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 16]), Some("image/jpeg"));
        assert_eq!(super::image_kind(b"RIFF\x24\0\0\0WEBPVP8 "), Some("image/webp"));
        // what could run as a page, and what is simply not a photo
        for other in [&b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"[..], b"<!DOCTYPE html>", b"GIF89a", b"RIFF\x24\0\0\0WAVEfmt ", b""] {
            assert_eq!(super::image_kind(other), None);
        }
    }

    #[test]
    fn only_the_page_opens_style_src() {
        assert!(super::csp("text/html").contains("style-src 'self' 'unsafe-inline'"));
        assert!(!super::csp("text/html").contains("script-src"));
        assert_eq!(super::csp("text/javascript"), "default-src 'self'; frame-ancestors 'none'");
        assert_eq!(super::csp("application/json"), "default-src 'self'; frame-ancestors 'none'");
    }

    #[test]
    fn mermaid_is_embedded_and_loaded_by_name() {
        assert!(super::MERMAID.contains("globalThis[\"mermaid\"]"));
        assert!(super::MERMAID_PATH.contains("11.12.0"));
        assert!(include_str!("../www/plan.js").contains(super::MERMAID_PATH));
    }

    fn oidc_config() -> super::Config {
        let dir = std::env::temp_dir().join(format!("cairn-plan-oidc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("config.json");
        let cfg = serde_json::json!({"trusted_proxies": ["127.0.0.1"], "require_groups": ["members"], "admin_groups": ["admins"],
            "public_url": "https://plan.example.org/", "oidc_issuer": "https://auth.example.org/", "oidc_client_id": "cairn-plan-mcp"});
        std::fs::write(&f, cfg.to_string()).unwrap();
        super::Config::load(f.to_str().unwrap())
    }

    #[test]
    fn access_token_runs_as_its_owner() {
        let cfg = oidc_config();
        let m = super::resource_metadata(&cfg).unwrap();
        assert_eq!(m["resource"], "https://plan.example.org/mcp");
        assert_eq!(m["authorization_servers"][0], "https://auth.example.org");
        assert_eq!(super::resource_metadata_url(&cfg), "https://plan.example.org/.well-known/oauth-protected-resource/mcp");
        let info = |aud: serde_json::Value, groups: &[&str]| {
            serde_json::json!({"aud": aud, "preferred_username": "Alice", "groups": groups, "name": "Alice B", "email": "j@example.org"})
        };
        let (who, name, email) = super::token_owner(&cfg, &info(serde_json::json!(["cairn-plan-mcp"]), &["members", "admins"])).unwrap();
        assert_eq!((who.login.as_str(), who.admin, name.as_str(), email.as_str()), ("alice", true, "Alice B", "j@example.org"));
        let (who, _, _) = super::token_owner(&cfg, &info(serde_json::json!("cairn-plan-mcp"), &["members"])).unwrap();
        assert!(!who.admin);
        // audience = the resource asked for (RFC 8707), read in the token (Authelia's JWT access tokens)
        let claims = super::jwt_claims("eyJhbGciOiJSUzI1NiJ9.eyJhdWQiOiBbImh0dHBzOi8vcGxhbi5leGFtcGxlLm9yZy9tY3AiXSwgImNsaWVudF9pZCI6ICJjYWlybi1wbGFuLW1jcCIsICJuIjogIsOpIn0.c2ln");
        assert_eq!((claims["aud"][0].as_str(), claims["client_id"].as_str(), claims["n"].as_str()), (Some("https://plan.example.org/mcp"), Some("cairn-plan-mcp"), Some("é")));
        assert!(super::jwt_claims("opaque-token").is_null());
        assert!(super::token_owner(&cfg, &info(serde_json::json!(["https://plan.example.org/mcp"]), &["members"])).is_ok());
        // a token issued to another application, someone outside require_groups, no login
        assert!(super::token_owner(&cfg, &info(serde_json::json!(["other-app"]), &["members"])).is_err());
        assert!(super::token_owner(&cfg, &info(serde_json::json!(["cairn-plan-mcp"]), &["staff"])).is_err());
        assert!(super::token_owner(&cfg, &serde_json::json!({"aud": "cairn-plan-mcp", "groups": ["members"]})).is_err());
    }

    #[test]
    fn config_keys_documented() {
        let doc = include_str!("../docs/configuration.md");
        for k in super::KEYS {
            assert!(doc.contains(&format!("`{k}`")), "docs/configuration.md does not describe \"{k}\"");
        }
    }
}

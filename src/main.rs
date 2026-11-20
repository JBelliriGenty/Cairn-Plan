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

/// Every key read from the configuration file (checked against the docs by a test).
const KEYS: [&str; 19] = [
    "listen", "trusted_proxies", "require_groups", "admin_groups", "db", "lang", "mcp_keys_file", "mcp_login", "webhook_secrets_file",
    "public_url", "smtp_host", "smtp_port", "smtp_credentials_file", "mail_from", "ntfy_url", "ntfy_token_file", "ntfy_topics",
    "agent_hook_url", "agent_hook_secret_file",
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
    /// The account every MCP call runs as.
    mcp_login: String,
    /// (name, secret) pairs that may sign a forge webhook; empty: no webhook.
    webhook_secrets: Vec<(String, String)>,
    /// Where notifications go besides the page (notify.rs).
    outside: notify::Outside,
    /// (address, secret) of the webhook told about tickets to work on; empty address: none.
    agent_hook: (String, String),
    /// Folder of the files attached to tickets: "files", next to the database.
    files: std::path::PathBuf,
}

/// Largest file that can be attached to a ticket.
const MAX_FILE: u64 = 25 * 1024 * 1024;

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
            webhook_secrets: secrets_file(&v, "webhook_secrets_file"),
            files: std::path::Path::new(&s("db", "/var/lib/cairn-plan/plan.db")).with_file_name("files"),
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
        assert!(!cfg.trusted_proxies.is_empty(), "trusted_proxies is empty: nobody could sign in");
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

/// MCP endpoint (mcp.rs): a key in "Authorization: Bearer", no proxy
/// identity. The key says which client calls; the account is always mcp_login.
fn serve_mcp(cfg: &Config, mut r: Request) {
    if cfg.mcp_keys.is_empty() {
        return respond(r, 404, "not found".into(), "text/plain");
    }
    let sent = header(&r, "Authorization");
    let key = sent.strip_prefix("Bearer ").unwrap_or("");
    let Some((client, _)) = cfg.mcp_keys.iter().find(|(_, k)| ct_eq(k, key)) else {
        let _ = r.respond(
            Response::from_string("unauthorized").with_status_code(401).with_header(Header::from_bytes("WWW-Authenticate", "Bearer").unwrap()),
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
    let who = Actor { login: cfg.mcp_login.clone(), admin: false };
    if req["method"] == "tools/call" {
        let a = &req["params"]["arguments"];
        let target = a["id"].as_str().or(a["project"].as_str()).unwrap_or("");
        eprintln!("mcp {client}: {} {target}", req["params"]["name"].as_str().unwrap_or("?"));
    }
    let c = DB.get().expect("database").lock().unwrap_or_else(|e| e.into_inner());
    store::seen(&c, &who.login, "", "");
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

fn respond(r: Request, code: u16, body: String, ctype: &str) {
    let h = |k: &str, v: &str| Header::from_bytes(k, v).unwrap();
    let _ = r.respond(
        Response::from_string(body)
            .with_status_code(code)
            .with_header(h("Content-Type", &format!("{ctype}; charset=utf-8")))
            .with_header(h("Cache-Control", "no-store"))
            .with_header(h("X-Content-Type-Options", "nosniff"))
            .with_header(h("Referrer-Policy", "no-referrer"))
            .with_header(h("Content-Security-Policy", "default-src 'self'; frame-ancestors 'none'")),
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
    fn config_keys_documented() {
        let doc = include_str!("../docs/configuration.md");
        for k in super::KEYS {
            assert!(doc.contains(&format!("`{k}`")), "docs/configuration.md does not describe \"{k}\"");
        }
    }
}

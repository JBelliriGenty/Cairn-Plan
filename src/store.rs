//! Data and rules: projects, members, tickets, comments, links, history.
//!
//! Roles on a project: reader (read, comment), contributor (create and edit
//! tickets), owner (approve, manage members). An instance admin is owner
//! everywhere. Approval rule: only an owner moves a ticket out of "backlog"
//! or into "todo", and only an owner changes the text of an approved ticket:
//! what was approved is what gets built. A contributor may also ask an owner
//! something only they can do or decide: a ticket created "waiting",
//! assigned to that owner, never approved until the owner says so.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub type R<T> = Result<T, (u16, String)>;

pub struct Actor {
    pub login: String,
    pub admin: bool,
}

pub const STATES: [&str; 6] = ["backlog", "todo", "in_progress", "waiting", "done", "cancelled"];
pub const PRIORITIES: [&str; 5] = ["none", "low", "medium", "high", "urgent"];
const ROLES: [&str; 3] = ["reader", "contributor", "owner"];
/// Ticket fields a client may set, in the order they are checked.
const FIELDS: [&str; 10] = ["title", "description", "state", "priority", "assignee", "start_date", "end_date", "milestone", "agent", "wake_date"];
/// Sub-tickets a parent may have; a sub-ticket has none of its own.
const MAX_CHILDREN: i64 = 8;

const SCHEMA: &str = "
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS users(
  login TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', email TEXT NOT NULL DEFAULT '', seen INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS projects(
  id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  space TEXT NOT NULL DEFAULT '', next_number INTEGER NOT NULL DEFAULT 1, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS members(
  project INTEGER NOT NULL REFERENCES projects(id), login TEXT NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('reader','contributor','owner')), PRIMARY KEY(project, login));
CREATE TABLE IF NOT EXISTS tickets(
  id INTEGER PRIMARY KEY, project INTEGER NOT NULL REFERENCES projects(id), number INTEGER NOT NULL,
  title TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'backlog',
  priority TEXT NOT NULL DEFAULT 'none', assignee TEXT NOT NULL DEFAULT '',
  start_date TEXT NOT NULL DEFAULT '', end_date TEXT NOT NULL DEFAULT '',
  milestone TEXT NOT NULL DEFAULT '', ext TEXT NOT NULL DEFAULT '',
  author TEXT NOT NULL, created INTEGER NOT NULL, updated INTEGER NOT NULL, UNIQUE(project, number));
CREATE TABLE IF NOT EXISTS comments(
  id INTEGER PRIMARY KEY, ticket INTEGER NOT NULL REFERENCES tickets(id), author TEXT NOT NULL,
  body TEXT NOT NULL, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS links(
  id INTEGER PRIMARY KEY, ticket INTEGER NOT NULL REFERENCES tickets(id), url TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS events(
  id INTEGER PRIMARY KEY, ticket INTEGER NOT NULL REFERENCES tickets(id), actor TEXT NOT NULL,
  field TEXT NOT NULL, old TEXT NOT NULL, new TEXT NOT NULL, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS notifications(
  id INTEGER PRIMARY KEY, login TEXT NOT NULL, ticket INTEGER NOT NULL REFERENCES tickets(id), actor TEXT NOT NULL,
  kind TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '', at INTEGER NOT NULL,
  read INTEGER NOT NULL DEFAULT 0, sent INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS files(
  id INTEGER PRIMARY KEY, ticket INTEGER NOT NULL REFERENCES tickets(id), name TEXT NOT NULL, size INTEGER NOT NULL,
  author TEXT NOT NULL, created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS worklogs(
  id INTEGER PRIMARY KEY, ticket INTEGER NOT NULL REFERENCES tickets(id), login TEXT NOT NULL, minutes INTEGER NOT NULL,
  at INTEGER NOT NULL, note TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS hook_outbox(
  id INTEGER PRIMARY KEY, ticket INTEGER NOT NULL REFERENCES tickets(id), event TEXT NOT NULL, actor TEXT NOT NULL,
  at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS runs(
  run_id TEXT PRIMARY KEY, login TEXT NOT NULL, agent TEXT NOT NULL, source TEXT NOT NULL, host TEXT NOT NULL DEFAULT '',
  project TEXT NOT NULL DEFAULT '', ticket TEXT NOT NULL DEFAULT '', task TEXT NOT NULL, state TEXT NOT NULL,
  model TEXT NOT NULL DEFAULT '', tokens_in INTEGER, tokens_out INTEGER, cost_usd REAL, started INTEGER NOT NULL,
  ended INTEGER, parent TEXT NOT NULL DEFAULT '', result TEXT NOT NULL DEFAULT '', updated INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS runs_started ON runs(started);
";

pub fn open(path: &str) -> Connection {
    let c = Connection::open(path).expect("cannot open the database");
    c.execute_batch(SCHEMA).expect("cannot create the tables");
    // databases created by 0.1.0: the column is added once, the error "already there" is ignored
    let _ = c.execute("ALTER TABLE projects ADD COLUMN space TEXT NOT NULL DEFAULT ''", []);
    // 0.4.0: milestone (a family of tickets), ext (id of the ticket in an imported draft)
    let _ = c.execute("ALTER TABLE tickets ADD COLUMN milestone TEXT NOT NULL DEFAULT ''", []);
    let _ = c.execute("ALTER TABLE tickets ADD COLUMN ext TEXT NOT NULL DEFAULT ''", []);
    // 0.7.0: whether someone wants notifications by e-mail too
    let _ = c.execute("ALTER TABLE users ADD COLUMN mail INTEGER NOT NULL DEFAULT 1", []);
    // 0.10.0: the ntfy topic someone chose for their notifications
    let _ = c.execute("ALTER TABLE users ADD COLUMN ntfy_topic TEXT NOT NULL DEFAULT ''", []);
    // 0.15.0: the role of the agent a ticket is for, and the parent of a sub-ticket (0: none)
    let _ = c.execute("ALTER TABLE tickets ADD COLUMN agent TEXT NOT NULL DEFAULT ''", []);
    let _ = c.execute("ALTER TABLE tickets ADD COLUMN parent INTEGER NOT NULL DEFAULT 0", []);
    // 0.18.0: what an hour of someone costs, in cents (0: not set)
    let _ = c.execute("ALTER TABLE users ADD COLUMN rate INTEGER NOT NULL DEFAULT 0", []);
    // 0.19.0: the version of someone's profile photo (when it was set, in milliseconds; 0: none)
    let _ = c.execute("ALTER TABLE users ADD COLUMN photo INTEGER NOT NULL DEFAULT 0", []);
    // 0.25.0: the day a ticket wakes up; until then it is out of "for me" and no agent takes it
    let _ = c.execute("ALTER TABLE tickets ADD COLUMN wake_date TEXT NOT NULL DEFAULT ''", []);
    c
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn db<T>(r: rusqlite::Result<T>) -> R<T> {
    r.map_err(|e| {
        eprintln!("database: {e}");
        (500, "database error".to_string())
    })
}

fn bad<T>(m: &str) -> R<T> {
    Err((400, m.into()))
}

fn denied<T>(m: &str) -> R<T> {
    Err((403, m.into()))
}

fn text(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(|s| s.trim().to_string())
}

pub fn valid_login(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

pub fn valid_key(s: &str) -> bool {
    (2..=10).contains(&s.len())
        && s.as_bytes()[0].is_ascii_uppercase()
        && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
        && (1..=12).contains(&s[5..7].parse::<u8>().unwrap_or(0))
        && (1..=31).contains(&s[8..10].parse::<u8>().unwrap_or(0))
}

fn valid_url(s: &str) -> bool {
    (s.starts_with("https://") || s.starts_with("http://"))
        && s.len() <= 2000
        && !s.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<' || c == '>')
}

fn rank(role: &str) -> u8 {
    match role {
        "reader" => 1,
        "contributor" => 2,
        "owner" => 3,
        _ => 0,
    }
}

/// (project id, rank). A project the actor is not a member of does not exist for them.
fn access(c: &Connection, a: &Actor, key: &str) -> R<(i64, u8)> {
    let id: Option<i64> = db(c.query_row("SELECT id FROM projects WHERE key = ?1", params![key], |r| r.get(0)).optional())?;
    let Some(id) = id else { return Err((404, "no such project".into())) };
    if a.admin {
        return Ok((id, 4));
    }
    let role: Option<String> =
        db(c.query_row("SELECT role FROM members WHERE project = ?1 AND login = ?2", params![id, a.login], |r| r.get(0)).optional())?;
    match role {
        Some(r) => Ok((id, rank(&r))),
        None => Err((404, "no such project".into())),
    }
}

fn role_name(n: u8) -> &'static str {
    match n {
        4 => "admin",
        3 => "owner",
        2 => "contributor",
        _ => "reader",
    }
}

/// "SHOP-12" -> (project id, rank, ticket id)
fn ticket(c: &Connection, a: &Actor, id: &str) -> R<(i64, u8, i64)> {
    let missing = || (404, "no such ticket".to_string());
    let (key, n) = id.rsplit_once('-').ok_or_else(missing)?;
    let n: i64 = n.parse().map_err(|_| missing())?;
    let (pid, rank) = access(c, a, key).map_err(|_| missing())?;
    let tid: Option<i64> =
        db(c.query_row("SELECT id FROM tickets WHERE project = ?1 AND number = ?2", params![pid, n], |r| r.get(0)).optional())?;
    Ok((pid, rank, tid.ok_or_else(missing)?))
}

fn event(c: &Connection, tid: i64, actor: &str, field: &str, old: &str, new: &str) -> R<()> {
    db(c.execute(
        "INSERT INTO events(ticket, actor, field, old, new, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![tid, actor, field, old, new, now()],
    ))?;
    db(c.execute("UPDATE tickets SET updated = ?1 WHERE id = ?2", params![now(), tid]))?;
    Ok(())
}

/// One field of a ticket, checked before it is written.
fn check(c: &Connection, pid: i64, field: &str, val: &str) -> R<()> {
    let ok = match field {
        "title" => (1..=200).contains(&val.chars().count()),
        "description" => val.len() <= 20_000,
        "state" => STATES.contains(&val),
        "priority" => PRIORITIES.contains(&val),
        "assignee" => {
            val.is_empty()
                || db(c.query_row("SELECT 1 FROM members WHERE project = ?1 AND login = ?2", params![pid, val], |_| Ok(())).optional())?
                    .is_some()
        }
        "start_date" | "end_date" | "wake_date" => val.is_empty() || valid_date(val),
        "milestone" => val.chars().count() <= 80,
        "agent" => val.is_empty() || (val.len() <= 20 && valid_login(val)),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err((400, format!("invalid {field}")))
    }
}

/* ---------- users ---------- */

/// A request that carries no name or address (the MCP server, a proxy that
/// does not send them) keeps what is known: notifications need the address.
pub fn seen(c: &Connection, login: &str, name: &str, email: &str) {
    let _ = c.execute(
        "INSERT INTO users(login, name, email, seen) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(login) DO UPDATE SET name = CASE WHEN ?2 != '' THEN ?2 ELSE name END,
           email = CASE WHEN ?3 != '' THEN ?3 ELSE email END, seen = ?4",
        params![login, name, email, now()],
    );
}

/// Who has a profile photo: login -> version of the photo. The page shows
/// initials for everyone else, without asking for a file that does not exist.
pub fn photos(c: &Connection) -> R<Value> {
    let mut st = db(c.prepare("SELECT login, photo FROM users WHERE photo > 0"))?;
    let mut out = serde_json::Map::new();
    for row in db(st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))))? {
        let (login, version) = db(row)?;
        out.insert(login, json!(version));
    }
    Ok(Value::Object(out))
}

/// Notes that an account has a profile photo (on) or none any more, and
/// answers its version (0: none). Everyone decides for their own account; an
/// administrator decides for the accounts of the agents, which never sign in
/// to the page, and can remove the photo of anyone (never choose it). The
/// file itself is kept by main.rs.
pub fn set_photo(c: &Connection, a: &Actor, login: &str, agents: &[String], on: bool) -> R<i64> {
    if !valid_login(login) {
        return bad("invalid login");
    }
    let agent = agents.iter().any(|l| l == login);
    if login != a.login && !(a.admin && (agent || !on)) {
        return denied(if agent { "only an administrator can set the photo of an agent" } else { "everyone chooses their own photo" });
    }
    let version = if on { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(1) } else { 0 };
    // an agent that never called yet has no line
    db(c.execute(
        "INSERT INTO users(login, seen, photo) VALUES (?1, 0, ?2) ON CONFLICT(login) DO UPDATE SET photo = ?2",
        params![login, version],
    ))?;
    Ok(version)
}

/* ---------- projects ---------- */

pub fn projects(c: &Connection, a: &Actor) -> R<Value> {
    let mut st = db(c.prepare(
        "SELECT p.key, p.name, p.description, COALESCE(m.role, ''), p.space,
                (SELECT COUNT(*) FROM tickets t WHERE t.project = p.id AND t.state NOT IN ('done', 'cancelled')),
                (SELECT COUNT(*) FROM tickets t WHERE t.project = p.id)
         FROM projects p LEFT JOIN members m ON m.project = p.id AND m.login = ?1
         WHERE ?2 OR m.role IS NOT NULL ORDER BY p.space = '', p.space, p.name",
    ))?;
    let rows = db(st.query_map(params![a.login, a.admin], |r| {
        let role: String = r.get(3)?;
        Ok(json!({
            "key": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "description": r.get::<_, String>(2)?,
            "role": if a.admin { "admin".to_string() } else { role },
            "space": r.get::<_, String>(4)?, "open": r.get::<_, i64>(5)?, "total": r.get::<_, i64>(6)?,
        }))
    }))?;
    Ok(Value::Array(db(rows.collect())?))
}

pub fn create_project(c: &Connection, a: &Actor, v: &Value) -> R<Value> {
    if !a.admin {
        return denied("only an administrator can create a project");
    }
    let (key, name) = (text(v, "key").unwrap_or_default(), text(v, "name").unwrap_or_default());
    let description = text(v, "description").unwrap_or_default();
    let space = text(v, "space").unwrap_or_default();
    if space.chars().count() > 60 {
        return bad("invalid space");
    }
    if !valid_key(&key) {
        return bad("invalid key");
    }
    if !(1..=100).contains(&name.chars().count()) || description.len() > 2000 {
        return bad("invalid name");
    }
    if db(c.query_row("SELECT 1 FROM projects WHERE key = ?1", params![key], |_| Ok(())).optional())?.is_some() {
        return Err((409, "this key is already used".into()));
    }
    db(c.execute("INSERT INTO projects(key, name, description, space, created) VALUES (?1, ?2, ?3, ?4, ?5)", params![key, name, description, space, now()]))?;
    db(c.execute("INSERT INTO members(project, login, role) VALUES (?1, ?2, 'owner')", params![c.last_insert_rowid(), a.login]))?;
    Ok(json!({ "key": key }))
}

pub fn project(c: &Connection, a: &Actor, key: &str) -> R<Value> {
    let (pid, rank) = access(c, a, key)?;
    let (name, description, space): (String, String, String) = db(c.query_row(
        "SELECT name, description, space FROM projects WHERE id = ?1",
        params![pid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ))?;
    let mut st = db(c.prepare(
        "SELECT m.login, m.role, COALESCE(u.name, '') FROM members m LEFT JOIN users u ON u.login = m.login
         WHERE m.project = ?1 ORDER BY m.login",
    ))?;
    let members: Vec<Value> = db(db(st.query_map(params![pid], |r| {
        Ok(json!({ "login": r.get::<_, String>(0)?, "role": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)? }))
    }))?
    .collect())?;
    let mut st = db(c.prepare(
        "SELECT t.number, t.title, t.state, t.priority, t.assignee, t.start_date, t.end_date, t.updated, t.milestone, t.agent,
                COALESCE((SELECT number FROM tickets p WHERE p.id = t.parent), 0),
                (SELECT COUNT(*) FROM tickets k WHERE k.parent = t.id AND k.state != 'cancelled'),
                (SELECT COUNT(*) FROM tickets k WHERE k.parent = t.id AND k.state = 'done'), t.wake_date
         FROM tickets t WHERE t.project = ?1 ORDER BY t.number DESC",
    ))?;
    let tickets: Vec<Value> = db(db(st.query_map(params![pid], |r| {
        let parent: i64 = r.get(10)?;
        Ok(json!({
            "id": format!("{key}-{}", r.get::<_, i64>(0)?), "title": r.get::<_, String>(1)?,
            "state": r.get::<_, String>(2)?, "priority": r.get::<_, String>(3)?, "assignee": r.get::<_, String>(4)?,
            "start_date": r.get::<_, String>(5)?, "end_date": r.get::<_, String>(6)?, "updated": r.get::<_, i64>(7)?,
            "milestone": r.get::<_, String>(8)?, "agent": r.get::<_, String>(9)?,
            "parent": if parent > 0 { format!("{key}-{parent}") } else { String::new() },
            "progress": {"total": r.get::<_, i64>(11)?, "done": r.get::<_, i64>(12)?},
            "wake_date": r.get::<_, String>(13)?,
        }))
    }))?
    .collect())?;
    Ok(json!({
        "key": key, "name": name, "description": description, "space": space, "role": role_name(rank),
        "members": members, "tickets": tickets,
    }))
}

/// Name and description: project owners. Space (the company or area the
/// project is filed under): administrators only, like creating a project.
// ponytail: a space is a name on the project, spaces = the names in use. A
// spaces table when a space needs members or settings of its own.
pub fn update_project(c: &Connection, a: &Actor, key: &str, v: &Value) -> R<Value> {
    let (pid, rank) = access(c, a, key)?;
    if rank < 3 {
        return denied("only a project owner can change the project");
    }
    if let Some(name) = text(v, "name") {
        if !(1..=100).contains(&name.chars().count()) {
            return bad("invalid name");
        }
        db(c.execute("UPDATE projects SET name = ?1 WHERE id = ?2", params![name, pid]))?;
    }
    if let Some(description) = text(v, "description") {
        if description.len() > 2000 {
            return bad("invalid name");
        }
        db(c.execute("UPDATE projects SET description = ?1 WHERE id = ?2", params![description, pid]))?;
    }
    if let Some(space) = text(v, "space") {
        let current: String = db(c.query_row("SELECT space FROM projects WHERE id = ?1", params![pid], |r| r.get(0)))?;
        if space != current {
            if !a.admin {
                return denied("only an administrator can move a project to another space");
            }
            if space.chars().count() > 60 {
                return bad("invalid space");
            }
            db(c.execute("UPDATE projects SET space = ?1 WHERE id = ?2", params![space, pid]))?;
        }
    }
    Ok(json!({ "ok": true }))
}

/// {"login": "...", "role": "reader" | "contributor" | "owner" | null (remove)}
pub fn set_member(c: &Connection, a: &Actor, key: &str, v: &Value) -> R<Value> {
    let (pid, rank) = access(c, a, key)?;
    if rank < 3 {
        return denied("only a project owner can manage members");
    }
    let login = text(v, "login").unwrap_or_default();
    if !valid_login(&login) {
        return bad("invalid login");
    }
    match v["role"].as_str() {
        None => {
            db(c.execute("DELETE FROM members WHERE project = ?1 AND login = ?2", params![pid, login]))?;
            // a removed member no longer holds tickets
            db(c.execute("UPDATE tickets SET assignee = '' WHERE project = ?1 AND assignee = ?2", params![pid, login]))?;
        }
        Some(role) if ROLES.contains(&role) => {
            db(c.execute(
                "INSERT INTO members(project, login, role) VALUES (?1, ?2, ?3)
                 ON CONFLICT(project, login) DO UPDATE SET role = ?3",
                params![pid, login, role],
            ))?;
        }
        Some(_) => return bad("invalid role"),
    }
    Ok(json!({ "ok": true }))
}

/* ---------- tickets ---------- */

pub fn create_ticket(c: &Connection, a: &Actor, key: &str, v: &Value) -> R<Value> {
    let (pid, rank) = access(c, a, key)?;
    if rank < 2 {
        return denied("readers cannot create tickets");
    }
    let mut vals: Vec<String> = Vec::new();
    for f in FIELDS {
        let val = text(v, f).unwrap_or_else(|| match f {
            "state" => "backlog".into(),
            "priority" => "none".into(),
            _ => String::new(),
        });
        check(c, pid, f, &val)?;
        vals.push(val);
    }
    let parent = match text(v, "parent").filter(|p| !p.is_empty()) {
        Some(p) => Some(parent_of(c, pid, &a.login, &p)?),
        None => None,
    };
    // a sub-ticket of an approved parent, created by whoever took the parent, is approved with it
    let delegated = vals[2] == "todo" && parent.is_some_and(|(_, covered)| covered);
    if vals[2] != "backlog" && rank < 3 && !delegated {
        if vals[2] != "waiting" {
            return denied("only a project owner can approve a ticket");
        }
        let owner = db(c
            .query_row("SELECT 1 FROM members WHERE project = ?1 AND login = ?2 AND role = 'owner'", params![pid, vals[4]], |_| Ok(()))
            .optional())?;
        if owner.is_none() {
            return denied("a question goes to a project owner: assign it to one");
        }
    }
    db(c.execute("UPDATE projects SET next_number = next_number + 1 WHERE id = ?1", params![pid]))?;
    let n: i64 = db(c.query_row("SELECT next_number - 1 FROM projects WHERE id = ?1", params![pid], |r| r.get(0)))?;
    db(c.execute(
        "INSERT INTO tickets(project, number, title, description, state, priority, assignee, start_date, end_date, milestone,
                             author, created, updated, agent, parent, wake_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13, ?14, ?15)",
        params![pid, n, vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6], vals[7], a.login, now(), vals[8],
                parent.map_or(0, |(id, _)| id), vals[9]],
    ))?;
    let tid = c.last_insert_rowid();
    event(c, tid, &a.login, "created", "", &vals[2])?;
    if vals[2] == "todo" {
        announce(c, tid, "ticket.approved", &a.login)?;
    }
    if !vals[4].is_empty() {
        notify(c, tid, &a.login, "assigned", "", Some(&vals[4]))?;
    }
    Ok(json!({ "id": format!("{key}-{n}") }))
}

/// The author of a question closes it: report as a comment, state "done".
/// Allowed only on a ticket this account opened already waiting (its first
/// event, "created" -> "waiting", is by this account) and still "waiting".
pub fn close_question(c: &Connection, a: &Actor, id: &str, report: &str) -> R<Value> {
    let (_, rank, tid) = ticket(c, a, id)?;
    let (state, opened_by): (String, Option<String>) = db(c.query_row(
        "SELECT t.state, (SELECT actor FROM events WHERE ticket = t.id AND field = 'created' AND new = 'waiting' ORDER BY id LIMIT 1)
         FROM tickets t WHERE t.id = ?1",
        params![tid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if opened_by.as_deref() != Some(a.login.as_str()) {
        return denied("only the account that asked this question can close it");
    }
    if state != "waiting" {
        return Err((409, "only a question still in state \"waiting\" can be closed".into()));
    }
    if rank < 2 {
        return denied("readers cannot edit tickets"); // before the comment: no orphan
    }
    comment(c, a, id, &json!({"body": report}))?;
    update_ticket(c, a, id, &json!({"state": "done"}))?;
    Ok(json!({ "ok": true }))
}

/// Who approved a ticket last, and the id of that event.
fn approval_event(c: &Connection, tid: i64) -> R<Option<(String, i64)>> {
    db(c.query_row(
        "SELECT actor, id FROM events WHERE ticket = ?1 AND field IN ('state', 'created') AND new = 'todo' ORDER BY id DESC LIMIT 1",
        params![tid],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional())
}

/// The parent named for a new sub-ticket: (its id, whether its approval
/// covers the sub-ticket). It does when the parent is in progress, taken by
/// whoever creates the sub-ticket, approved, and its text unchanged since by
/// anyone else: what was approved is split, never widened.
fn parent_of(c: &Connection, pid: i64, login: &str, id: &str) -> R<(i64, bool)> {
    let (key, n) = id.rsplit_once('-').unwrap_or(("", ""));
    let found: Option<(i64, i64, String, String, i64)> = db(c
        .query_row(
            "SELECT t.id, t.parent, t.state, t.assignee, t.project FROM tickets t JOIN projects p ON p.id = t.project
             WHERE p.key = ?1 AND t.number = ?2",
            params![key, n.parse::<i64>().unwrap_or(0)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional())?;
    let Some((tid, grand, state, assignee, _)) = found.filter(|f| f.4 == pid) else {
        return bad("the parent must be a ticket of the same project");
    };
    if grand != 0 {
        return bad("a sub-ticket cannot have sub-tickets");
    }
    if state == "done" || state == "cancelled" {
        return bad("the parent is closed");
    }
    let count: i64 = db(c.query_row("SELECT COUNT(*) FROM tickets WHERE parent = ?1", params![tid], |r| r.get(0)))?;
    if count >= MAX_CHILDREN {
        return bad("this ticket has as many sub-tickets as it may");
    }
    let covered = state == "in_progress"
        && assignee == login
        && match approval_event(c, tid)? {
            Some((by, at)) => {
                db(c.query_row(
                    "SELECT COUNT(*) FROM events WHERE ticket = ?1 AND id > ?2 AND field IN ('title', 'description') AND actor != ?3",
                    params![tid, at, by],
                    |r| r.get::<_, i64>(0),
                ))? == 0
            }
            None => false,
        };
    Ok((tid, covered))
}

pub fn update_ticket(c: &Connection, a: &Actor, id: &str, v: &Value) -> R<Value> {
    let (pid, rank, tid) = ticket(c, a, id)?;
    if rank < 2 {
        return denied("readers cannot edit tickets");
    }
    let old: Vec<String> = db(c.query_row(
        "SELECT title, description, state, priority, assignee, start_date, end_date, milestone, agent, wake_date FROM tickets WHERE id = ?1",
        params![tid],
        |r| (0..FIELDS.len()).map(|i| r.get(i)).collect(),
    ))?;
    let state = old[2].as_str();
    let mut changes: Vec<(&str, &str, String)> = Vec::new();
    for (i, f) in FIELDS.iter().enumerate() {
        let Some(new) = text(v, f).filter(|n| *n != old[i]) else { continue };
        check(c, pid, f, &new)?;
        if rank < 3 {
            // the agent role says who builds it: part of what was approved
            if (*f == "title" || *f == "description" || *f == "agent" || *f == "wake_date") && state != "backlog" {
                return denied("only a project owner can change the text of an approved ticket");
            }
            if *f == "state" && (new == "todo" || state == "backlog") {
                return denied("only a project owner can approve a ticket");
            }
        }
        changes.push((*f, old[i].as_str(), new));
    }
    for (f, was, new) in &changes {
        // f comes from FIELDS, never from the request
        db(c.execute(&format!("UPDATE tickets SET {f} = ?1 WHERE id = ?2"), params![new, tid]))?;
        event(c, tid, &a.login, f, was, new)?;
        match *f {
            "state" => {
                // work that starts is not news: the approval, the question and the end are
                if new != "in_progress" {
                    notify(c, tid, &a.login, "state", new, None)?;
                }
                if new == "todo" {
                    announce(c, tid, "ticket.approved", &a.login)?;
                }
                // a part closed, by an agent or a person: the part that waited for it may go
                if new == "done" || new == "cancelled" {
                    let parent: i64 = db(c.query_row("SELECT parent FROM tickets WHERE id = ?1", params![tid], |r| r.get(0)))?;
                    if parent != 0 {
                        announce(c, tid, "ticket.closed", &a.login)?;
                    }
                }
                if new == "cancelled" {
                    // cancelling a parent stops its sub-tickets: one gesture keeps the owner in charge
                    let mut st = db(c.prepare("SELECT id, state FROM tickets WHERE parent = ?1 AND state NOT IN ('done', 'cancelled')"))?;
                    let open: Vec<(i64, String)> = db(db(st.query_map(params![tid], |r| Ok((r.get(0)?, r.get(1)?))))?.collect())?;
                    for (kid, was) in open {
                        db(c.execute("UPDATE tickets SET state = 'cancelled' WHERE id = ?1", params![kid]))?;
                        event(c, kid, &a.login, "state", &was, "cancelled")?;
                    }
                }
            }
            "assignee" if !new.is_empty() => notify(c, tid, &a.login, "assigned", "", Some(new))?,
            _ => {}
        }
    }
    Ok(json!({ "changed": changes.len() }))
}

/// The open tickets that wait on someone, in all their projects and spaces:
/// those assigned to them, and the questions asked on a ticket they approved
/// (it stays assigned to whoever asked) until they answer in a comment.
/// First what waits for an answer, then the newest. "since": when the ticket
/// entered its state; "last": the last comment, which may change what is asked.
pub fn mine(c: &Connection, a: &Actor) -> R<Value> {
    // who approved a ticket last: the answer a waiting ticket waits for is theirs (see comment())
    let approver =
        |of: &str| format!("(SELECT actor FROM events WHERE ticket = {of} AND field IN ('state', 'created') AND new = 'todo' ORDER BY id DESC LIMIT 1)");
    let last = |col: &str| format!("COALESCE((SELECT {col} FROM comments WHERE ticket = t.id ORDER BY id DESC LIMIT 1), '')");
    let mut st = db(c.prepare(&format!(
        "SELECT p.key || '-' || t.number, t.title, t.state, t.priority, p.key, p.name, p.space, t.author, t.updated, t.assignee,
                COALESCE((SELECT MAX(at) FROM events WHERE ticket = t.id AND field IN ('state', 'created') AND new = t.state), t.updated),
                {}, {}
         FROM tickets t JOIN projects p ON p.id = t.project
         WHERE t.state NOT IN ('done', 'cancelled') AND (t.wake_date = '' OR t.wake_date <= date('now'))
           AND (t.assignee = ?1 OR (t.state = 'waiting' AND ?1 IN ({}, {}) AND ?1 != {}))
         ORDER BY t.state != 'waiting', t.updated DESC",
        last("author"),
        last("substr(body, 1, 300)"),
        approver("t.id"),
        approver("t.parent"),
        last("author"),
    )))?;
    let rows = db(st.query_map(params![a.login], |r| {
        Ok(json!({
            "id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?, "state": r.get::<_, String>(2)?,
            "priority": r.get::<_, String>(3)?, "project": r.get::<_, String>(4)?, "project_name": r.get::<_, String>(5)?,
            "space": r.get::<_, String>(6)?, "author": r.get::<_, String>(7)?, "updated": r.get::<_, i64>(8)?,
            "assignee": r.get::<_, String>(9)?, "since": r.get::<_, i64>(10)?,
            "last": {"author": r.get::<_, String>(11)?, "body": r.get::<_, String>(12)?},
        }))
    }))?;
    Ok(Value::Array(db(rows.collect())?))
}

pub fn ticket_view(c: &Connection, a: &Actor, id: &str) -> R<Value> {
    let (_, rank, tid) = ticket(c, a, id)?;
    let mut t: Value = db(c.query_row(
        "SELECT t.title, t.description, t.state, t.priority, t.assignee, t.start_date, t.end_date, t.author, t.created, t.updated,
                t.milestone, t.agent, COALESCE((SELECT number FROM tickets p WHERE p.id = t.parent), 0), t.wake_date
         FROM tickets t WHERE t.id = ?1",
        params![tid],
        |r| {
            let parent: i64 = r.get(12)?;
            Ok(json!({
                "title": r.get::<_, String>(0)?, "description": r.get::<_, String>(1)?, "state": r.get::<_, String>(2)?,
                "priority": r.get::<_, String>(3)?, "assignee": r.get::<_, String>(4)?, "start_date": r.get::<_, String>(5)?,
                "end_date": r.get::<_, String>(6)?, "author": r.get::<_, String>(7)?, "created": r.get::<_, i64>(8)?,
                "updated": r.get::<_, i64>(9)?, "milestone": r.get::<_, String>(10)?, "agent": r.get::<_, String>(11)?,
                "wake_date": r.get::<_, String>(13)?,
                "parent": if parent > 0 { format!("{}-{parent}", id.rsplit_once('-').map_or("", |(k, _)| k)) } else { String::new() },
            }))
        },
    ))?;
    let key = id.rsplit_once('-').map_or("", |(k, _)| k);
    let mut st = db(c.prepare("SELECT number, title, state, agent, assignee FROM tickets WHERE parent = ?1 ORDER BY number"))?;
    let children: Vec<Value> = db(db(st.query_map(params![tid], |r| {
        Ok(json!({
            "id": format!("{key}-{}", r.get::<_, i64>(0)?), "title": r.get::<_, String>(1)?, "state": r.get::<_, String>(2)?,
            "agent": r.get::<_, String>(3)?, "assignee": r.get::<_, String>(4)?,
        }))
    }))?
    .collect())?;
    let counted = children.iter().filter(|k| k["state"] != "cancelled").count();
    let done = children.iter().filter(|k| k["state"] == "done").count();
    t["progress"] = json!({"total": counted, "done": done});
    t["children"] = json!(children);
    let mut st = db(c.prepare("SELECT author, body, created FROM comments WHERE ticket = ?1 ORDER BY id"))?;
    let comments: Vec<Value> = db(db(st.query_map(params![tid], |r| {
        Ok(json!({ "author": r.get::<_, String>(0)?, "body": r.get::<_, String>(1)?, "at": r.get::<_, i64>(2)? }))
    }))?
    .collect())?;
    let mut st = db(c.prepare("SELECT id, url, title FROM links WHERE ticket = ?1 ORDER BY id"))?;
    let links: Vec<Value> = db(db(st.query_map(params![tid], |r| {
        Ok(json!({ "id": r.get::<_, i64>(0)?, "url": r.get::<_, String>(1)?, "title": r.get::<_, String>(2)? }))
    }))?
    .collect())?;
    let mut st = db(c.prepare("SELECT actor, field, old, new, at FROM events WHERE ticket = ?1 ORDER BY id DESC"))?;
    let events: Vec<Value> = db(db(st.query_map(params![tid], |r| {
        Ok(json!({
            "actor": r.get::<_, String>(0)?, "field": r.get::<_, String>(1)?, "old": r.get::<_, String>(2)?,
            "new": r.get::<_, String>(3)?, "at": r.get::<_, i64>(4)?,
        }))
    }))?
    .collect())?;
    t["id"] = json!(id);
    t["role"] = json!(role_name(rank));
    t["comments"] = json!(comments);
    t["links"] = json!(links);
    let mut st = db(c.prepare("SELECT id, name, size, author, created FROM files WHERE ticket = ?1 ORDER BY id"))?;
    let files: Vec<Value> = db(db(st.query_map(params![tid], |r| {
        Ok(json!({
            "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "size": r.get::<_, i64>(2)?,
            "author": r.get::<_, String>(3)?, "at": r.get::<_, i64>(4)?,
        }))
    }))?
    .collect())?;
    t["files"] = json!(files);
    let mut st = db(c.prepare("SELECT id, login, minutes, at, note FROM worklogs WHERE ticket = ?1 ORDER BY at DESC, id DESC"))?;
    let work: Vec<Value> = db(db(st.query_map(params![tid], |r| {
        Ok(json!({
            "id": r.get::<_, i64>(0)?, "login": r.get::<_, String>(1)?, "minutes": r.get::<_, i64>(2)?,
            "at": r.get::<_, i64>(3)?, "note": r.get::<_, String>(4)?,
        }))
    }))?
    .collect())?;
    let logged: i64 = work.iter().filter_map(|w| w["minutes"].as_i64()).sum();
    t["time"] = json!({ "logged": logged, "states": state_seconds(&events, &t, now()) });
    t["work"] = json!(work);
    t["events"] = json!(events);
    Ok(t)
}

pub fn comment(c: &Connection, a: &Actor, id: &str, v: &Value) -> R<Value> {
    let (_, _, tid) = ticket(c, a, id)?;
    let body = text(v, "body").unwrap_or_default();
    if body.is_empty() || body.len() > 10_000 {
        return bad("invalid comment");
    }
    db(c.execute("INSERT INTO comments(ticket, author, body, created) VALUES (?1, ?2, ?3, ?4)", params![tid, a.login, body, now()]))?;
    db(c.execute("UPDATE tickets SET updated = ?1 WHERE id = ?2", params![now(), tid]))?;
    notify(c, tid, &a.login, "comment", &body, None)?;
    // the answer a waiting ticket waits for is the one of whoever approved it, or approved its
    // parent (a sub-ticket is approved by the lead who cut it, on the parent's approval)
    let (waiting, parent): (bool, i64) =
        db(c.query_row("SELECT state = 'waiting', parent FROM tickets WHERE id = ?1", params![tid], |r| Ok((r.get(0)?, r.get(1)?))))?;
    let mut approvers = vec![approval_event(c, tid)?];
    if parent != 0 {
        approvers.push(approval_event(c, parent)?);
    }
    if waiting && approvers.into_iter().flatten().any(|(by, _)| by == a.login) {
        announce(c, tid, "ticket.answered", &a.login)?;
    }
    Ok(json!({ "ok": true }))
}

/// {"url": "...", "title": "..."} adds a link, {"delete": <id>} removes one.
pub fn link(c: &Connection, a: &Actor, id: &str, v: &Value) -> R<Value> {
    let (_, rank, tid) = ticket(c, a, id)?;
    if rank < 2 {
        return denied("readers cannot edit tickets");
    }
    if let Some(lid) = v["delete"].as_i64() {
        let url: Option<String> =
            db(c.query_row("SELECT url FROM links WHERE id = ?1 AND ticket = ?2", params![lid, tid], |r| r.get(0)).optional())?;
        let Some(url) = url else { return Err((404, "no such link".into())) };
        db(c.execute("DELETE FROM links WHERE id = ?1", params![lid]))?;
        event(c, tid, &a.login, "link", &url, "")?;
        return Ok(json!({ "ok": true }));
    }
    let (url, title) = (text(v, "url").unwrap_or_default(), text(v, "title").unwrap_or_default());
    if !valid_url(&url) || title.chars().count() > 200 {
        return bad("invalid link");
    }
    db(c.execute("INSERT INTO links(ticket, url, title) VALUES (?1, ?2, ?3)", params![tid, url, title]))?;
    event(c, tid, &a.login, "link", "", &url)?;
    Ok(json!({ "ok": true }))
}

/// A link set by a forge webhook (hooks.rs). No member is behind it, so no
/// access check: an unknown ticket or a bad address is skipped (false). One
/// link per address: a later event about it only refreshes its label.
pub fn hook_link(c: &Connection, id: &str, url: &str, title: &str) -> R<bool> {
    let Some((key, n)) = id.rsplit_once('-') else { return Ok(false) };
    let tid: Option<i64> = db(c
        .query_row(
            "SELECT t.id FROM tickets t JOIN projects p ON p.id = t.project WHERE p.key = ?1 AND t.number = ?2",
            params![key, n.parse::<i64>().unwrap_or(0)],
            |r| r.get(0),
        )
        .optional())?;
    let Some(tid) = tid else { return Ok(false) };
    if !valid_url(url) {
        return Ok(false);
    }
    let title: String = title.chars().take(200).collect();
    if db(c.execute("UPDATE links SET title = ?1 WHERE ticket = ?2 AND url = ?3", params![title, tid, url]))? == 0 {
        db(c.execute("INSERT INTO links(ticket, url, title) VALUES (?1, ?2, ?3)", params![tid, url, title]))?;
        event(c, tid, "forge", "link", "", url)?;
    }
    Ok(true)
}

/* ---------- time ---------- */

/// Seconds a ticket spent in each state, read from its history (newest
/// event first). The state it is in counts up to now, a final one (done,
/// cancelled) does not count.
fn state_seconds(events: &[Value], ticket: &Value, now: i64) -> Value {
    let mut spans: Vec<(&str, i64)> = events
        .iter()
        .rev()
        .filter(|e| e["field"] == "created" || e["field"] == "state")
        .filter_map(|e| Some((e["new"].as_str()?, e["at"].as_i64()?)))
        .collect();
    if spans.is_empty() {
        spans.push((ticket["state"].as_str().unwrap_or(""), ticket["created"].as_i64().unwrap_or(now)));
    }
    let mut total = serde_json::Map::new();
    for (i, (state, since)) in spans.iter().enumerate() {
        let until = match spans.get(i + 1) {
            Some((_, next)) => *next,
            None if *state == "done" || *state == "cancelled" => *since,
            None => now,
        };
        let so_far = total.get(*state).and_then(Value::as_i64).unwrap_or(0);
        total.insert(state.to_string(), json!(so_far + (until - since).max(0)));
    }
    Value::Object(total)
}

/// What a time report covers: one project, or the projects of a space the viewer sees.
pub enum Scope<'a> {
    Project(&'a str),
    Space(&'a str),
}

/// The cost an agent reported at the end of its comment: a last line
/// "<role> · <model> · 0.46 $". None for any other comment.
fn reported_usd(body: &str) -> Option<f64> {
    let last = body.trim_end().lines().last()?.trim();
    let amount = last.strip_suffix('$')?.trim_end();
    let (_, value) = amount.rsplit_once(" · ")?;
    value.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

/// Time of a project or of a space, for the "Time" page: each ticket with
/// its clock (seconds per state, when it was closed) and the cost its
/// agents reported, then each entry of logged time. The page adds them up
/// as it shows them (per week, person, milestone, project). `agents` are
/// the accounts of the agents (MCP accounts): only their comments carry a
/// cost, and the page tells their time from the time of people. The hourly
/// rates of people ("rates", cents per hour) are given to an administrator
/// and to an owner of the project (of every project shown, for a space).
pub fn time_report(c: &Connection, a: &Actor, scope: Scope, agents: &[String]) -> R<Value> {
    let mut money = a.admin;
    let (kind, name, space, projects): (&str, String, String, Vec<(i64, String, String)>) = match scope {
        Scope::Project(key) => {
            let (pid, rank) = access(c, a, key)?;
            money = rank >= 3;
            let (pname, pspace): (String, String) =
                db(c.query_row("SELECT name, space FROM projects WHERE id = ?1", params![pid], |r| Ok((r.get(0)?, r.get(1)?))))?;
            ("project", pname.clone(), pspace, vec![(pid, key.to_string(), pname)])
        }
        Scope::Space(space) => {
            let mut st = db(c.prepare(
                "SELECT p.id, p.key, p.name, COALESCE(m.role, '') FROM projects p
                 LEFT JOIN members m ON m.project = p.id AND m.login = ?1
                 WHERE p.space = ?3 AND (?2 OR m.role IS NOT NULL) ORDER BY p.name",
            ))?;
            let found: Vec<(i64, String, String, String)> = db(db(st
                .query_map(params![a.login, a.admin, space], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))))?
            .collect())?;
            if found.is_empty() {
                return Err((404, "no such space".into()));
            }
            money = a.admin || found.iter().all(|(_, _, _, role)| rank(role) >= 3);
            ("space", space.to_string(), space.to_string(), found.into_iter().map(|(id, key, name, _)| (id, key, name)).collect())
        }
    };
    let now = now();
    let (mut tickets, mut work, mut costs) = (Vec::new(), Vec::new(), Vec::new());
    for (pid, key, _) in &projects {
        let mut st = db(c.prepare(
            "SELECT t.id, t.number, t.title, t.state, t.milestone, t.agent, t.assignee, t.created,
                    COALESCE((SELECT number FROM tickets p WHERE p.id = t.parent), 0)
             FROM tickets t WHERE t.project = ?1 ORDER BY t.number",
        ))?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(i64, i64, String, String, String, String, String, i64, i64)> = db(db(st.query_map(params![pid], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?))
        }))?
        .collect())?;
        for (tid, number, title, state, milestone, agent, assignee, created, parent) in rows {
            let mut st = db(c.prepare(
                "SELECT field, new, at FROM events WHERE ticket = ?1 AND field IN ('created', 'state') ORDER BY id DESC",
            ))?;
            let events: Vec<Value> = db(db(st.query_map(params![tid], |r| {
                Ok(json!({ "field": r.get::<_, String>(0)?, "new": r.get::<_, String>(1)?, "at": r.get::<_, i64>(2)? }))
            }))?
            .collect())?;
            let ticket = json!({ "state": state, "created": created });
            let closed = if state == "done" || state == "cancelled" {
                events.iter().find(|e| e["field"] == "state").and_then(|e| e["at"].as_i64()).unwrap_or(created)
            } else {
                0
            };
            let id = format!("{key}-{number}");
            // what each run of an agent cost: the last line of its report, dated by the comment
            let mut st = db(c.prepare("SELECT author, body, created FROM comments WHERE ticket = ?1 ORDER BY id"))?;
            let mut usd = 0.0;
            for row in db(st.query_map(params![tid], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))))? {
                let (author, body, at) = db(row)?;
                if let Some(v) = reported_usd(&body).filter(|_| agents.contains(&author)) {
                    usd += v;
                    costs.push(json!({ "ticket": id, "login": author, "usd": v, "at": at }));
                }
            }
            let mut st = db(c.prepare("SELECT login, minutes, at, note FROM worklogs WHERE ticket = ?1 ORDER BY at, id"))?;
            let entries: Vec<Value> = db(db(st.query_map(params![tid], |r| {
                Ok(json!({
                    "ticket": id, "login": r.get::<_, String>(0)?, "minutes": r.get::<_, i64>(1)?, "at": r.get::<_, i64>(2)?,
                    "note": r.get::<_, String>(3)?,
                }))
            }))?
            .collect())?;
            work.extend(entries);
            tickets.push(json!({
                "id": id, "project": key, "title": title, "state": state, "milestone": milestone, "agent": agent,
                "assignee": assignee, "parent": if parent > 0 { format!("{key}-{parent}") } else { String::new() },
                "created": created, "closed": closed, "states": state_seconds(&events, &ticket, now),
                "usd": (usd * 100.0).round() / 100.0,
            }));
        }
    }
    let mut agents: Vec<&String> = agents.iter().collect();
    agents.sort();
    agents.dedup();
    let mut rates = serde_json::Map::new();
    if money {
        let mut st = db(c.prepare("SELECT login, rate FROM users WHERE rate > 0"))?;
        for row in db(st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))))? {
            let (login, rate) = db(row)?;
            rates.insert(login, json!(rate));
        }
    }
    let currency = settings(c).remove("currency").filter(|v| !v.is_empty()).unwrap_or_else(|| "€".to_string());
    Ok(json!({
        "scope": kind, "name": name, "space": space, "now": now, "agents": agents,
        "money": money, "rates": rates, "currency": currency,
        "projects": projects.iter().map(|(_, key, name)| json!({ "key": key, "name": name })).collect::<Vec<_>>(),
        "tickets": tickets, "work": work, "costs": costs,
    }))
}

/// {"login": "lea", "rate": 6500} sets what an hour of someone costs, in cents
/// (0: not set); {"currency": "€"} sets the sign shown after an amount. For
/// an administrator.
pub fn set_rate(c: &Connection, a: &Actor, v: &Value) -> R<Value> {
    if !a.admin {
        return denied("only an administrator can set an hourly rate");
    }
    if let Some(currency) = text(v, "currency") {
        if currency.is_empty() || currency.chars().count() > 8 || currency.chars().any(char::is_control) {
            return bad("invalid currency");
        }
        db(c.execute(
            "INSERT INTO settings(key, value) VALUES ('currency', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
            params![currency],
        ))?;
    }
    if let Some(login) = text(v, "login") {
        let rate = v["rate"].as_i64().unwrap_or(-1);
        // a million cents: ten thousand an hour at most
        if !(0..=1_000_000).contains(&rate) {
            return bad("invalid rate");
        }
        if db(c.execute("UPDATE users SET rate = ?1 WHERE login = ?2", params![rate, login]))? == 0 {
            return Err((404, "no such person".into()));
        }
    } else if !v["currency"].is_string() {
        return bad("invalid request");
    }
    Ok(json!({ "ok": true }))
}

/// {"minutes": 90, "day": "2026-11-18", "note": "..."} logs time someone
/// worked on a ticket (day and note optional). {"delete": <id>} removes an
/// entry: one's own, or anyone's for a project owner.
pub fn log_work(c: &Connection, a: &Actor, id: &str, v: &Value) -> R<Value> {
    let (_, rank, tid) = ticket(c, a, id)?;
    if rank < 2 {
        return denied("readers cannot edit tickets");
    }
    if let Some(wid) = v["delete"].as_i64() {
        let found: Option<(String, i64)> = db(c
            .query_row("SELECT login, minutes FROM worklogs WHERE id = ?1 AND ticket = ?2", params![wid, tid], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional())?;
        let Some((login, minutes)) = found else { return Err((404, "no such entry".into())) };
        if login != a.login && rank < 3 {
            return denied("only a project owner can remove the time of someone else");
        }
        db(c.execute("DELETE FROM worklogs WHERE id = ?1", params![wid]))?;
        event(c, tid, &a.login, "work", &minutes.to_string(), "")?;
        return Ok(json!({ "ok": true }));
    }
    let minutes = v["minutes"].as_i64().unwrap_or(0);
    let note = text(v, "note").unwrap_or_default();
    // 6000 minutes: one entry is at most a hundred hours
    if !(1..=6000).contains(&minutes) || note.chars().count() > 500 {
        return bad("invalid time");
    }
    let at = match text(v, "day").filter(|d| !d.is_empty()) {
        Some(day) => epoch(&day).ok_or_else(|| (400, "invalid day".to_string()))?,
        None => now(),
    };
    db(c.execute(
        "INSERT INTO worklogs(ticket, login, minutes, at, note) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![tid, a.login, minutes, at, note],
    ))?;
    event(c, tid, &a.login, "work", "", &minutes.to_string())?;
    Ok(json!({ "ok": true }))
}

/* ---------- attached files ---------- */

/// The name a file is kept under: its last path segment, without control characters.
fn file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    last.chars().filter(|ch| !ch.is_control()).collect::<String>().trim().chars().take(200).collect()
}

/// Records a file attached to a ticket and returns its number. The bytes
/// are kept by the caller, under that number: never under a name someone typed.
pub fn add_file(c: &Connection, a: &Actor, id: &str, name: &str, size: i64) -> R<i64> {
    let (_, rank, tid) = ticket(c, a, id)?;
    if rank < 2 {
        return denied("readers cannot edit tickets");
    }
    let name = file_name(name);
    if name.is_empty() {
        return bad("invalid file name");
    }
    db(c.execute(
        "INSERT INTO files(ticket, name, size, author, created) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![tid, name, size, a.login, now()],
    ))?;
    let fid = c.last_insert_rowid();
    event(c, tid, &a.login, "file", "", &name)?;
    Ok(fid)
}

/// The name of a file of a ticket, for someone who may read the ticket.
pub fn file(c: &Connection, a: &Actor, id: &str, fid: i64) -> R<String> {
    let (_, _, tid) = ticket(c, a, id)?;
    db(c.query_row("SELECT name FROM files WHERE id = ?1 AND ticket = ?2", params![fid, tid], |r| r.get(0)).optional())?
        .ok_or_else(|| (404, "no such file".to_string()))
}

/// {"delete": <number>} forgets a file and returns its number, for the caller to remove the bytes.
pub fn delete_file(c: &Connection, a: &Actor, id: &str, v: &Value) -> R<i64> {
    let (_, rank, tid) = ticket(c, a, id)?;
    if rank < 2 {
        return denied("readers cannot edit tickets");
    }
    let Some(fid) = v["delete"].as_i64() else { return bad("invalid request") };
    let name = file(c, a, id, fid)?;
    db(c.execute("DELETE FROM files WHERE id = ?1", params![fid]))?;
    event(c, tid, &a.login, "file", &name, "")?;
    Ok(fid)
}

/* ---------- notifications ---------- */

/// A notice to send outside the page (notify.rs).
pub struct Notice {
    pub login: String,
    pub email: String,
    /// The person wants notices by e-mail.
    pub mail: bool,
    /// The ntfy topic the person chose; empty: none.
    pub topic: String,
    /// Ticket id, e.g. SHOP-51.
    pub ticket: String,
    pub title: String,
    pub actor: String,
    /// "comment" (detail: its first words), "state" (detail: the new state), "assigned".
    pub kind: String,
    pub detail: String,
    /// It waits on the person: a question asked to them, a ticket that wakes.
    /// Only such a notice is pushed to a phone.
    pub question: bool,
}

/// Tells the people who take part in a ticket (its author, its assignee,
/// those who commented) what `actor` did, the actor excepted. `only`: that
/// person alone (an assignment). People who are no longer members of the
/// project are left out.
fn notify(c: &Connection, tid: i64, actor: &str, kind: &str, detail: &str, only: Option<&str>) -> R<()> {
    let detail: String = detail.chars().take(300).collect();
    db(c.execute(
        "INSERT INTO notifications(login, ticket, actor, kind, detail, at)
         SELECT who, ?1, ?2, ?3, ?4, ?5 FROM (
           SELECT author AS who FROM tickets WHERE id = ?1
           UNION SELECT assignee FROM tickets WHERE id = ?1
           UNION SELECT author FROM comments WHERE ticket = ?1)
         WHERE who != '' AND who != ?2 AND (?6 IS NULL OR who = ?6)
           AND who IN (SELECT login FROM members WHERE project = (SELECT project FROM tickets WHERE id = ?1))",
        params![tid, actor, kind, detail, now(), only],
    ))?;
    Ok(())
}

/// The last notices of someone, newest first, and how many are unread.
pub fn notifications(c: &Connection, a: &Actor) -> R<Value> {
    let unread: i64 = db(c.query_row("SELECT COUNT(*) FROM notifications WHERE login = ?1 AND read = 0", params![a.login], |r| r.get(0)))?;
    let mut st = db(c.prepare(
        "SELECT n.id, p.key || '-' || t.number, t.title, n.actor, n.kind, n.detail, n.at, n.read
         FROM notifications n JOIN tickets t ON t.id = n.ticket JOIN projects p ON p.id = t.project
         WHERE n.login = ?1 ORDER BY n.id DESC LIMIT 50",
    ))?;
    let items: Vec<Value> = db(db(st.query_map(params![a.login], |r| {
        Ok(json!({
            "id": r.get::<_, i64>(0)?, "ticket": r.get::<_, String>(1)?, "title": r.get::<_, String>(2)?,
            "actor": r.get::<_, String>(3)?, "kind": r.get::<_, String>(4)?, "detail": r.get::<_, String>(5)?,
            "at": r.get::<_, i64>(6)?, "read": r.get::<_, i64>(7)? != 0,
        }))
    }))?
    .collect())?;
    Ok(json!({ "unread": unread, "items": items }))
}

/// {"read": <id>} one notice, {"read": "all"} every one, {"ticket": "SHOP-1"} those of a ticket.
pub fn read_notifications(c: &Connection, a: &Actor, v: &Value) -> R<Value> {
    let n = if let Some(id) = v["ticket"].as_str() {
        let (_, _, tid) = ticket(c, a, id)?;
        db(c.execute("UPDATE notifications SET read = 1 WHERE login = ?1 AND ticket = ?2", params![a.login, tid]))?
    } else if v["read"] == "all" {
        db(c.execute("UPDATE notifications SET read = 1 WHERE login = ?1", params![a.login]))?
    } else if let Some(id) = v["read"].as_i64() {
        db(c.execute("UPDATE notifications SET read = 1 WHERE login = ?1 AND id = ?2", params![a.login, id]))?
    } else {
        return bad("invalid request");
    };
    Ok(json!({ "read": n }))
}

/// Whether someone wants notices by e-mail too (yes until they say no).
pub fn wants_mail(c: &Connection, login: &str) -> bool {
    c.query_row("SELECT mail FROM users WHERE login = ?1", params![login], |r| r.get::<_, i64>(0)).map(|m| m != 0).unwrap_or(true)
}

/// The ntfy topic someone chose for their notifications; empty: none.
pub fn ntfy_topic(c: &Connection, login: &str) -> String {
    c.query_row("SELECT ntfy_topic FROM users WHERE login = ?1", params![login], |r| r.get(0)).unwrap_or_default()
}

fn valid_topic(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// What someone chose for their own notifications: {"mail": false} stops
/// the e-mails, {"ntfy_topic": "plan-lea"} sets the topic of their pushes
/// (empty: none). The topic must start with the prefix the administrator set.
pub fn set_prefs(c: &Connection, a: &Actor, v: &Value) -> R<Value> {
    if let Some(on) = v["mail"].as_bool() {
        db(c.execute("UPDATE users SET mail = ?1 WHERE login = ?2", params![on, a.login]))?;
    }
    if let Some(topic) = text(v, "ntfy_topic") {
        let prefix = settings(c).remove("ntfy_prefix").unwrap_or_default();
        if !topic.is_empty() && !(valid_topic(&topic) && topic.starts_with(&prefix)) {
            return bad("invalid topic");
        }
        db(c.execute("UPDATE users SET ntfy_topic = ?1 WHERE login = ?2", params![topic, a.login]))?;
    }
    if !v["mail"].is_boolean() && !v["ntfy_topic"].is_string() {
        return bad("invalid request");
    }
    Ok(json!({ "ok": true }))
}

/* ---------- outgoing webhook ---------- */

/// Something an agent may act on (hooks.rs sends it): a ticket was approved
/// ("ticket.approved"), or whoever approved a waiting ticket answered in a
/// comment ("ticket.answered").
pub struct Call {
    pub event: String,
    /// Ticket id, e.g. SHOP-51.
    pub ticket: String,
    pub project: String,
    pub title: String,
    pub actor: String,
    pub at: i64,
}

fn announce(c: &Connection, tid: i64, event: &str, actor: &str) -> R<()> {
    db(c.execute("INSERT INTO hook_outbox(ticket, event, actor, at) VALUES (?1, ?2, ?3, ?4)", params![tid, event, actor, now()]))?;
    Ok(())
}

/// The calls not yet sent. They are removed: each is handed over once,
/// whatever becomes of it.
pub fn hook_outbox(c: &Connection) -> Vec<Call> {
    let Ok(mut st) = c.prepare(
        "SELECT h.event, p.key || '-' || t.number, p.key, t.title, h.actor, h.at
         FROM hook_outbox h JOIN tickets t ON t.id = h.ticket JOIN projects p ON p.id = t.project ORDER BY h.id",
    ) else {
        return Vec::new();
    };
    let calls: Vec<Call> = st
        .query_map([], |r| {
            Ok(Call { event: r.get(0)?, ticket: r.get(1)?, project: r.get(2)?, title: r.get(3)?, actor: r.get(4)?, at: r.get(5)? })
        })
        .map(|found| found.filter_map(Result::ok).collect())
        .unwrap_or_default();
    let _ = c.execute("DELETE FROM hook_outbox", []);
    calls
}

/* ---------- wake-up date ---------- */

/// True if the ticket sleeps: its wake-up date is still to come.
pub fn asleep(c: &Connection, a: &Actor, id: &str) -> R<bool> {
    let (_, _, tid) = ticket(c, a, id)?;
    db(c.query_row("SELECT wake_date > date('now') FROM tickets WHERE id = ?1", params![tid], |r| r.get(0)))
}

/// Wakes the tickets whose date has come: the date is cleared, the people of
/// the ticket are told, and an approved ticket is announced again, so that
/// an agent takes it now. Returns how many woke.
pub fn wake(c: &Connection) -> usize {
    let due: Vec<(i64, String, String)> = c
        .prepare("SELECT id, wake_date, state FROM tickets WHERE wake_date != '' AND wake_date <= date('now')")
        .and_then(|mut st| {
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default();
    for (tid, date, state) in &due {
        let _ = c.execute("UPDATE tickets SET wake_date = '' WHERE id = ?1", params![tid]);
        if state == "done" || state == "cancelled" {
            continue;
        }
        let _ = event(c, *tid, "cairn-plan", "wake_date", date, "");
        let _ = notify(c, *tid, "cairn-plan", "wake", "", None);
        if state == "todo" {
            let _ = announce(c, *tid, "ticket.approved", "cairn-plan");
        }
    }
    due.len()
}

/* ---------- search ---------- */

/// The text around the first match of `q` in `text`, on one line.
fn excerpt(text: &str, q: &str) -> String {
    let (low, q) = (text.to_lowercase(), q.to_lowercase());
    let at = low.find(&q).map_or(0, |b| low[..b].chars().count());
    let from = at.saturating_sub(60);
    let cut: String = text.chars().skip(from).take(200).map(|ch| if ch.is_whitespace() { ' ' } else { ch }).collect();
    format!("{}{}", if from > 0 { "…" } else { "" }, cut.trim())
}

/// The tickets whose id, title, description or comments hold `q`, in the
/// projects someone sees (`key`: one project only), newest first, 50 at
/// most. "excerpt": the text around the match when it is not in the title.
/// ponytail: LIKE over every ticket, blind to case for ASCII only; FTS5 when the base is large.
pub fn search(c: &Connection, a: &Actor, q: &str, key: &str) -> R<Value> {
    let q = q.trim();
    if !(2..=100).contains(&q.chars().count()) {
        return bad("search for 2 to 100 characters");
    }
    // the signs of LIKE are read as themselves
    let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut st = db(c.prepare(
        "SELECT p.key || '-' || t.number, t.title, t.state, p.key, p.name, p.space, t.updated,
                CASE WHEN t.title LIKE ?2 ESCAPE '\\' OR (p.key || '-' || t.number) LIKE ?2 ESCAPE '\\' THEN ''
                     WHEN t.description LIKE ?2 ESCAPE '\\' THEN t.description
                     ELSE COALESCE((SELECT body FROM comments WHERE ticket = t.id AND body LIKE ?2 ESCAPE '\\' ORDER BY id DESC LIMIT 1), '') END
         FROM tickets t JOIN projects p ON p.id = t.project
         WHERE (?3 OR p.id IN (SELECT project FROM members WHERE login = ?1)) AND (?4 = '' OR p.key = ?4)
           AND ((p.key || '-' || t.number) LIKE ?2 ESCAPE '\\' OR t.title LIKE ?2 ESCAPE '\\' OR t.description LIKE ?2 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM comments WHERE ticket = t.id AND body LIKE ?2 ESCAPE '\\'))
         ORDER BY t.updated DESC, t.id DESC LIMIT 50",
    ))?;
    let rows = db(st.query_map(params![a.login, like, a.admin, key], |r| {
        let around: String = r.get(7)?;
        Ok(json!({
            "id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?, "state": r.get::<_, String>(2)?,
            "project": r.get::<_, String>(3)?, "project_name": r.get::<_, String>(4)?, "space": r.get::<_, String>(5)?,
            "updated": r.get::<_, i64>(6)?, "excerpt": if around.is_empty() { String::new() } else { excerpt(&around, q) },
        }))
    }))?;
    Ok(Value::Array(db(rows.collect())?))
}

/* ---------- runs of agents ---------- */

const RUN_STATES: [&str; 4] = ["running", "ok", "failed", "waiting"];
const RUN_COLS: &str = "run_id, login, agent, source, host, project, ticket, task, state, model, tokens_in, tokens_out, cost_usd, \
                        started, ended, parent, result, updated";
/// Fields a client may send, besides "run_id"; what a later send leaves out is kept.
const RUN_FIELDS: [&str; 15] = [
    "agent", "source", "host", "project", "ticket", "task", "state", "model", "tokens_in", "tokens_out", "cost_usd", "started", "ended",
    "parent", "result",
];

fn run_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "run_id": r.get::<_, String>(0)?, "login": r.get::<_, String>(1)?, "agent": r.get::<_, String>(2)?,
        "source": r.get::<_, String>(3)?, "host": r.get::<_, String>(4)?, "project": r.get::<_, String>(5)?,
        "ticket": r.get::<_, String>(6)?, "task": r.get::<_, String>(7)?, "state": r.get::<_, String>(8)?,
        "model": r.get::<_, String>(9)?, "tokens_in": r.get::<_, Option<i64>>(10)?, "tokens_out": r.get::<_, Option<i64>>(11)?,
        "cost_usd": r.get::<_, Option<f64>>(12)?, "started": r.get::<_, i64>(13)?, "ended": r.get::<_, Option<i64>>(14)?,
        "parent": r.get::<_, String>(15)?, "result": r.get::<_, String>(16)?, "updated": r.get::<_, i64>(17)?,
    }))
}

fn valid_ticket_id(s: &str) -> bool {
    s.rsplit_once('-').is_some_and(|(k, n)| valid_key(k) && (1..=9).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A short text of a run: absent is empty; a number, a line break or a text that is too long is refused.
fn run_text(m: &Value, key: &str, max: usize, required: bool) -> R<String> {
    let s = match &m[key] {
        Value::Null => "",
        Value::String(s) => s.trim(),
        _ => return bad(&format!("{key}: a text is expected")),
    };
    if s.chars().count() > max || s.chars().any(char::is_control) {
        return bad(&format!("{key}: at most {max} characters, on one line"));
    }
    if required && s.is_empty() {
        return bad(&format!("{key} is required"));
    }
    Ok(s.to_string())
}

/// An account reports a run of its own. A second report of the same
/// "run_id" updates it (what the new report leaves out is kept); one made by
/// another account is refused. Only a summary: never the instruction nor the
/// output of the agent, which may hold a secret. A run of a project is
/// reported by one of its contributors; the project is read from the ticket
/// when it is not given.
pub fn report_run(c: &Connection, a: &Actor, v: &Value) -> R<Value> {
    let run_id = run_text(v, "run_id", 100, true)?;
    let old: Option<Value> =
        db(c.query_row(&format!("SELECT {RUN_COLS} FROM runs WHERE run_id = ?1"), params![run_id], run_json).optional())?;
    if old.as_ref().is_some_and(|o| o["login"] != a.login.as_str()) {
        return denied("this run was reported by another account");
    }
    let mut m = old.unwrap_or_else(|| json!({}));
    for k in RUN_FIELDS {
        if !v[k].is_null() {
            m[k] = v[k].clone();
        }
    }
    let agent = run_text(&m, "agent", 60, true)?;
    let source = run_text(&m, "source", 30, true)?;
    if !valid_login(&source) {
        return bad("source: lower case letters, digits, . _ - (e.g. claude-code)");
    }
    let state = run_text(&m, "state", 20, true)?;
    if !RUN_STATES.contains(&state.as_str()) {
        return bad(&format!("state: one of {}", RUN_STATES.join(", ")));
    }
    let (host, model, result) = (run_text(&m, "host", 60, false)?, run_text(&m, "model", 60, false)?, run_text(&m, "result", 60, false)?);
    let (task, parent) = (run_text(&m, "task", 120, true)?, run_text(&m, "parent", 100, false)?);
    let (mut project, ticket) = (run_text(&m, "project", 10, false)?, run_text(&m, "ticket", 20, false)?);
    if !ticket.is_empty() && !valid_ticket_id(&ticket) {
        return bad("ticket: an id such as SHOP-12");
    }
    let of_ticket = ticket.rsplit_once('-').map_or("", |(k, _)| k);
    if project.is_empty() {
        project = of_ticket.to_string();
    }
    if !ticket.is_empty() && project != of_ticket {
        return bad("ticket: it belongs to another project");
    }
    if !project.is_empty() {
        if !valid_key(&project) {
            return bad("project: a project key such as SHOP");
        }
        if access(c, a, &project)?.1 < 2 {
            return denied("a run of a project is reported by one of its contributors");
        }
    }
    let int = |key: &str, max: i64| -> R<Option<i64>> {
        match &m[key] {
            Value::Null => Ok(None),
            n => n.as_i64().filter(|n| (0..=max).contains(n)).map(Some).ok_or((400, format!("{key}: a whole number, 0 to {max}"))),
        }
    };
    let (tokens_in, tokens_out) = (int("tokens_in", 1_000_000_000_000)?, int("tokens_out", 1_000_000_000_000)?);
    let cost = match &m["cost_usd"] {
        Value::Null => None,
        n => Some(n.as_f64().filter(|n| (0.0..=1_000_000.0).contains(n)).ok_or((400, "cost_usd: a number, 0 to 1000000".to_string()))?),
    };
    let started = int("started", i64::MAX)?.filter(|s| *s > 0).ok_or((400, "started: seconds since 1970".to_string()))?;
    let mut ended = int("ended", i64::MAX)?;
    if ended.is_some_and(|e| e < started) {
        return bad("ended: not before started");
    }
    if state == "running" && v["ended"].is_null() {
        ended = None; // sent again as going on: the end of an earlier report no longer holds
    } else if state != "running" && ended.is_none() {
        ended = Some(now().max(started));
    }
    db(c.execute(
        &format!(
            "INSERT INTO runs({RUN_COLS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT(run_id) DO UPDATE SET agent=?3, source=?4, host=?5, project=?6, ticket=?7, task=?8, state=?9, model=?10,
               tokens_in=?11, tokens_out=?12, cost_usd=?13, started=?14, ended=?15, parent=?16, result=?17, updated=?18"
        ),
        params![run_id, a.login, agent, source, host, project, ticket, task, state, model, tokens_in, tokens_out, cost, started, ended, parent, result, now()],
    ))?;
    db(c.query_row(&format!("SELECT {RUN_COLS} FROM runs WHERE run_id = ?1"), params![run_id], run_json))
}

/// The runs someone may see, newest first, 500 at most: those of the
/// projects they belong to; an administrator also sees the runs without a
/// project. `since`: a date "YYYY-MM-DD" (UTC) or seconds since 1970; a run
/// counts when it ended, or was last reported, since then. An empty `agent` or
/// `project` is no filter.
pub fn runs(c: &Connection, a: &Actor, since: &str, agent: &str, project: &str) -> R<Value> {
    let since: i64 = match since {
        "" => 0,
        s if s.bytes().all(|b| b.is_ascii_digit()) => s.parse().map_err(|_| (400, "since: a date or seconds".to_string()))?,
        s => epoch(s).map(|e| e - 43_200).ok_or((400, "since: a date YYYY-MM-DD or seconds".to_string()))?,
    };
    let mut st = db(c.prepare(&format!(
        "SELECT {RUN_COLS} FROM runs
         WHERE COALESCE(ended, updated) >= ?1 AND (?2 = '' OR agent = ?2) AND (?3 = '' OR project = ?3)
           AND (?4 OR (project != '' AND project IN
                (SELECT p.key FROM projects p JOIN members m ON m.project = p.id WHERE m.login = ?5)))
         ORDER BY started DESC, run_id LIMIT 500"
    )))?;
    let rows = db(st.query_map(params![since, agent, project, a.admin, a.login], run_json))?;
    Ok(Value::Array(db(rows.collect())?))
}

/* ---------- copies ---------- */

/// How many copies of the database `backup` keeps.
const COPIES: usize = 14;

/// A consistent copy of the database in `dir` (VACUUM INTO: safe while the
/// server runs), named by the second it is made. The newest are kept.
pub fn backup(c: &Connection, dir: &str) -> R<String> {
    std::fs::create_dir_all(dir).map_err(|e| (500, format!("cannot create {dir}: {e}")))?;
    let path = format!("{dir}/plan-{}.db", now());
    db(c.execute("VACUUM INTO ?1", params![path]))?;
    let mut copies: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("plan-") && n.ends_with(".db")))
        .collect();
    copies.sort();
    for old in copies.iter().rev().skip(COPIES) {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

/// A project in full, to keep outside the tool: the project and its members,
/// then every ticket as its page shows it (text, comments, links, history, time).
pub fn export_project(c: &Connection, a: &Actor, key: &str) -> R<Value> {
    let mut p = project(c, a, key)?;
    let ids: Vec<String> = p["tickets"].as_array().into_iter().flatten().filter_map(|k| k["id"].as_str().map(String::from)).collect();
    let full: Vec<Value> = ids.iter().map(|id| ticket_view(c, a, id)).collect::<R<_>>()?;
    p["tickets"] = json!(full);
    Ok(p)
}

/* ---------- settings ---------- */

/// What an administrator can set on the settings page. These win over the
/// configuration file, which keeps what is needed before anyone can sign in.
pub const SETTINGS: [&str; 11] = [
    "public_url", "smtp_host", "smtp_port", "smtp_user", "smtp_password", "mail_from", "ntfy_url", "ntfy_token", "ntfy_prefix",
    "agent_hook_url", "agent_hook_secret",
];
/// Never sent back to the page; an empty value leaves the saved one as it is.
pub const SECRETS: [&str; 3] = ["smtp_password", "ntfy_token", "agent_hook_secret"];

/// The settings saved from the page.
pub fn settings(c: &Connection) -> std::collections::HashMap<String, String> {
    let Ok(mut st) = c.prepare("SELECT key, value FROM settings") else { return Default::default() };
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default()
}

/// {"smtp_host": "...", ...}: saves the settings given, for an administrator.
pub fn save_settings(c: &Connection, a: &Actor, v: &Value) -> R<Value> {
    if !a.admin {
        return denied("only an administrator can change the settings");
    }
    for key in SETTINGS {
        let Some(val) = text(v, key) else { continue };
        if SECRETS.contains(&key) && val.is_empty() {
            continue;
        }
        let ok = val.chars().count() <= 300
            && !val.chars().any(char::is_control)
            && match key {
                "public_url" | "ntfy_url" | "agent_hook_url" => val.is_empty() || valid_url(&val),
                "agent_hook_secret" => val.len() >= 16,
                "smtp_port" => val.is_empty() || val.parse::<u16>().is_ok_and(|p| p > 0),
                "ntfy_prefix" => val.is_empty() || (valid_topic(&val) && val.len() <= 32),
                _ => true,
            };
        if !ok {
            return Err((400, format!("invalid {key}")));
        }
        db(c.execute("INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", params![key, val]))?;
    }
    Ok(json!({ "ok": true }))
}

/// Notices not yet sent outside the page. They are marked as sent: each is
/// handed over once, whatever becomes of it.
pub fn outbox(c: &Connection) -> Vec<Notice> {
    // a notice read more than 90 days ago is of no use any more
    let _ = c.execute("DELETE FROM notifications WHERE read = 1 AND at < ?1", params![now() - 90 * 86400]);
    let Ok(mut st) = c.prepare(
        "SELECT n.id, n.login, COALESCE(u.email, ''), COALESCE(u.mail, 1), p.key || '-' || t.number, t.title, n.actor, n.kind, n.detail,
                COALESCE(u.ntfy_topic, ''),
                (n.kind = 'state' AND n.detail = 'waiting') OR (n.kind = 'assigned' AND t.state = 'waiting') OR (n.kind = 'wake' AND n.login = t.assignee)
         FROM notifications n JOIN tickets t ON t.id = n.ticket JOIN projects p ON p.id = t.project
         LEFT JOIN users u ON u.login = n.login WHERE n.sent = 0 ORDER BY n.id LIMIT 200",
    ) else {
        return Vec::new();
    };
    let rows: Vec<(i64, Notice)> = st
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                Notice {
                    login: r.get(1)?,
                    email: r.get(2)?,
                    mail: r.get::<_, i64>(3)? != 0,
                    ticket: r.get(4)?,
                    title: r.get(5)?,
                    actor: r.get(6)?,
                    kind: r.get(7)?,
                    detail: r.get(8)?,
                    topic: r.get(9)?,
                    question: r.get(10)?,
                },
            ))
        })
        .map(|found| found.filter_map(Result::ok).collect())
        .unwrap_or_default();
    if let Some((last, _)) = rows.last() {
        let _ = c.execute("UPDATE notifications SET sent = 1 WHERE sent = 0 AND id <= ?1", params![last]);
    }
    rows.into_iter().map(|(_, n)| n).collect()
}

/* ---------- import of a history draft ---------- */

/// Seconds since 1970 for noon UTC of a "YYYY-MM-DD" (Howard Hinnant's
/// days-from-civil): the date a lot started becomes the date its ticket was created.
fn epoch(date: &str) -> Option<i64> {
    if !valid_date(date) {
        return None;
    }
    let (y, m, d): (i64, i64, i64) = (date[0..4].parse().ok()?, date[5..7].parse().ok()?, date[8..10].parse().ok()?);
    let y = if m <= 2 { y - 1 } else { y };
    let (era, yoe) = (y.div_euclid(400), y.rem_euclid(400));
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400 + 43_200)
}

/// Web address of a source named in a draft, when it has one: a pull
/// request, a commit, a Markdown document of the repository.
fn source_url(src: &str, depot: &str, infra: &str) -> Option<String> {
    if let Some(path) = src.strip_prefix("dépôt infra : ") {
        return (!infra.is_empty() && !path.contains(' ')).then(|| format!("{infra}/src/branch/main/{path}"));
    }
    if depot.is_empty() {
        return None;
    }
    if let Some(n) = src.strip_prefix("PR ") {
        return (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{depot}/pulls/{n}"));
    }
    if let Some(sha) = src.strip_prefix("commit ") {
        return ((7..=40).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("{depot}/commit/{sha}"));
    }
    (src.ends_with(".md") && src.bytes().all(|b| b.is_ascii_alphanumeric() || b"./-_".contains(&b)))
        .then(|| format!("{depot}/src/branch/main/{src}"))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn bullets(items: &[String]) -> String {
    items.iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")
}

fn section(out: &mut String, title: &str, body: &str) {
    if body.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(title);
    out.push('\n');
    out.push_str(body);
}

/// One ticket of a draft.
struct Entry {
    ext: String,
    title: String,
    description: String,
    state: &'static str,
    priority: &'static str,
    start: String,
    end: String,
    milestone: String,
    links: Vec<(String, String)>,
}

fn entries(draft: &Value) -> Vec<Entry> {
    let (depot, infra) = (text(draft, "depot").unwrap_or_default(), text(draft, "depot_infra").unwrap_or_default());
    let links = |sources: &[String]| -> Vec<(String, String)> {
        sources.iter().filter_map(|s| source_url(s, &depot, &infra).map(|u| (u, s.clone()))).collect()
    };
    let families: Vec<(String, String)> = draft["familles"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|n| (k.clone(), n.to_string()))).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    // lots, oldest first so that the ticket numbers follow the story
    let mut lots: Vec<(usize, &Value)> = draft["lots"].as_array().map(|a| a.iter().enumerate().collect()).unwrap_or_default();
    lots.sort_by_key(|(i, l)| (l["debut"].as_str().filter(|d| !d.is_empty()).unwrap_or("9999").to_string(), *i));
    for (i, l) in lots {
        let (jalon, titre) = (text(l, "jalon").unwrap_or_default(), text(l, "titre").unwrap_or_default());
        let sources = strings(&l["sources"]);
        let mut d = String::new();
        section(&mut d, "Objectif", &text(l, "objectif").unwrap_or_default());
        section(&mut d, "Choix et raisons", &bullets(&strings(&l["choix"])));
        section(&mut d, "Écarté", &bullets(&strings(&l["ecarte"])));
        section(&mut d, "Résultat", &text(l, "resultat").unwrap_or_default());
        section(&mut d, "Sources", &bullets(&sources));
        out.push(Entry {
            ext: format!("lot-{i:03}"),
            milestone: families
                .iter()
                .find(|(prefix, _)| !jalon.is_empty() && jalon.starts_with(prefix.as_str()))
                .map(|(_, n)| n.clone())
                .unwrap_or_default(),
            title: if jalon.is_empty() { titre } else { format!("{jalon} · {titre}") },
            description: d,
            state: match l["etat"].as_str() {
                Some("termine") => "done",
                Some("en_cours") => "in_progress",
                _ => "backlog",
            },
            priority: "none",
            start: text(l, "debut").unwrap_or_default(),
            end: text(l, "fin").unwrap_or_default(),
            links: links(&sources),
        });
    }
    for (i, dec) in draft["decisions"].as_array().map(|a| a.iter().enumerate().collect::<Vec<_>>()).unwrap_or_default() {
        let (date, source) = (text(dec, "date").unwrap_or_default(), text(dec, "source").unwrap_or_default());
        let mut d = String::new();
        section(&mut d, "Décision", &text(dec, "resume").unwrap_or_default());
        section(&mut d, "Source", &source);
        out.push(Entry {
            ext: format!("decision-{i:03}"),
            title: format!("Décision · {}", text(dec, "titre").unwrap_or_default()),
            description: d,
            state: "done",
            priority: "none",
            start: date.clone(),
            end: date,
            milestone: "Décisions".into(),
            links: links(&[source]),
        });
    }
    let open = strings(&draft["incertitudes"]);
    if !open.is_empty() {
        out.push(Entry {
            ext: "incertitudes".into(),
            title: "Points à confirmer sur l'historique repris".into(),
            description: format!(
                "Ce que les documents ne permettent pas d'établir. À confirmer ou corriger en commentaire.\n\n{}",
                bullets(&open)
            ),
            state: "todo",
            priority: "medium",
            start: String::new(),
            end: String::new(),
            milestone: String::new(),
            links: Vec::new(),
        });
    }
    out
}

/// Imports a history draft (`cairn-plan import`): one project, one ticket
/// per lot and per decision, one ticket for the open points. `login` owns
/// the project and holds the tickets. A replay creates nothing twice: it
/// brings titles, descriptions, dates, milestones and links in line with
/// the draft, and leaves what people change by hand (state, priority,
/// assignee, comments) alone.
pub fn import(c: &Connection, login: &str, space: &str, draft: &Value) -> R<String> {
    let name = text(draft, "projet").unwrap_or_default();
    let key: String = name.to_uppercase().chars().filter(|ch| ch.is_ascii_alphanumeric()).take(10).collect();
    if !valid_key(&key) || !valid_login(login) || space.chars().count() > 60 {
        return bad("invalid draft: project name, login or space");
    }
    let list = entries(draft);
    for e in &list {
        let dates_ok = [&e.start, &e.end].iter().all(|d| d.is_empty() || valid_date(d));
        if e.title.is_empty() || e.title.chars().count() > 200 || e.description.len() > 20_000 || !dates_ok || e.milestone.chars().count() > 80 {
            return Err((400, format!("invalid draft: {}", e.ext)));
        }
    }
    db(c.execute_batch("BEGIN"))?;
    let done = (|| -> R<String> {
        let found: Option<i64> = db(c.query_row("SELECT id FROM projects WHERE key = ?1", params![key], |r| r.get(0)).optional())?;
        let pid = match found {
            Some(id) => id,
            None => {
                db(c.execute(
                    "INSERT INTO projects(key, name, description, space, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![key, name, text(draft, "description").unwrap_or(format!("Historique et suivi de {name}.")), space, now()],
                ))?;
                c.last_insert_rowid()
            }
        };
        db(c.execute("INSERT OR IGNORE INTO members(project, login, role) VALUES (?1, ?2, 'owner')", params![pid, login]))?;
        // "membres": {"claude": "contributor"}, added once, never changed by a replay
        for (who, role) in draft["membres"].as_object().into_iter().flatten() {
            let role = role.as_str().unwrap_or_default();
            if !valid_login(who) || !ROLES.contains(&role) {
                return bad("invalid draft: membres");
            }
            db(c.execute("INSERT OR IGNORE INTO members(project, login, role) VALUES (?1, ?2, ?3)", params![pid, who, role]))?;
        }
        let (mut created, mut updated, mut linked) = (0, 0, 0);
        for e in &list {
            let found: Option<i64> =
                db(c.query_row("SELECT id FROM tickets WHERE project = ?1 AND ext = ?2", params![pid, e.ext], |r| r.get(0)).optional())?;
            let tid = match found {
                Some(tid) => {
                    db(c.execute(
                        "UPDATE tickets SET title = ?1, description = ?2, start_date = ?3, end_date = ?4, milestone = ?5 WHERE id = ?6",
                        params![e.title, e.description, e.start, e.end, e.milestone, tid],
                    ))?;
                    updated += 1;
                    tid
                }
                None => {
                    let at = epoch(&e.start).unwrap_or_else(now);
                    db(c.execute("UPDATE projects SET next_number = next_number + 1 WHERE id = ?1", params![pid]))?;
                    let n: i64 = db(c.query_row("SELECT next_number - 1 FROM projects WHERE id = ?1", params![pid], |r| r.get(0)))?;
                    db(c.execute(
                        "INSERT INTO tickets(project, number, title, description, state, priority, assignee, start_date, end_date,
                                             milestone, ext, author, created, updated)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?7, ?12, ?12)",
                        params![pid, n, e.title, e.description, e.state, e.priority, login, e.start, e.end, e.milestone, e.ext, at],
                    ))?;
                    let tid = c.last_insert_rowid();
                    db(c.execute(
                        "INSERT INTO events(ticket, actor, field, old, new, at) VALUES (?1, ?2, 'created', '', ?3, ?4)",
                        params![tid, login, e.state, at],
                    ))?;
                    created += 1;
                    tid
                }
            };
            for (url, title) in &e.links {
                if db(c.query_row("SELECT 1 FROM links WHERE ticket = ?1 AND url = ?2", params![tid, url], |_| Ok(())).optional())?.is_none() {
                    db(c.execute("INSERT INTO links(ticket, url, title) VALUES (?1, ?2, ?3)", params![tid, url, title]))?;
                    linked += 1;
                }
            }
        }
        Ok(format!("{key}: {created} tickets created, {updated} brought in line, {linked} links added"))
    })();
    db(c.execute_batch(if done.is_ok() { "COMMIT" } else { "ROLLBACK" }))?;
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(login: &str, admin: bool) -> Actor {
        Actor { login: login.into(), admin }
    }

    /// Project SHOP: jane admin and owner, lea contributor, tom reader, eve outside.
    fn setup() -> Connection {
        let c = open(":memory:");
        let jane = who("jane", true);
        create_project(&c, &jane, &json!({"key": "SHOP", "name": "Shop"})).unwrap();
        set_member(&c, &jane, "SHOP", &json!({"login": "lea", "role": "contributor"})).unwrap();
        set_member(&c, &jane, "SHOP", &json!({"login": "tom", "role": "reader"})).unwrap();
        c
    }

    #[test]
    fn an_approved_ticket_is_split_by_whoever_took_it_never_widened() {
        let c = setup();
        let (jane, lea) = (who("jane", true), who("lea", false));
        set_member(&c, &jane, "SHOP", &json!({"login": "max", "role": "contributor"})).unwrap();
        let max = who("max", false);
        let kid = |by: &Actor, parent: &str, state: &str| {
            create_ticket(&c, by, "SHOP", &json!({"title": "Part", "parent": parent, "state": state, "agent": "dev"}))
        };
        // SHOP-1 approved by jane, SHOP-2 only proposed
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Epic", "state": "todo"})).unwrap();
        create_ticket(&c, &lea, "SHOP", &json!({"title": "Idea"})).unwrap();
        // not taken yet: no approved sub-ticket, a proposal is fine
        assert_eq!(kid(&lea, "SHOP-1", "todo").unwrap_err().0, 403);
        update_ticket(&c, &lea, "SHOP-1", &json!({"state": "in_progress", "assignee": "lea"})).unwrap();
        // taken by lea: her sub-tickets are approved with it, not those of max
        assert_eq!(kid(&lea, "SHOP-1", "todo").unwrap()["id"], "SHOP-3");
        assert_eq!(kid(&max, "SHOP-1", "todo").unwrap_err().0, 403);
        // a proposal never covers anything; one level only; same project only
        assert_eq!(kid(&lea, "SHOP-2", "todo").unwrap_err().0, 403);
        assert_eq!(kid(&lea, "SHOP-3", "todo").unwrap_err().0, 400);
        assert_eq!(kid(&lea, "BLOG-1", "todo").unwrap_err().0, 400);
        // the sub-ticket is approved and frozen, its role too
        let t = ticket_view(&c, &lea, "SHOP-3").unwrap();
        assert_eq!((t["state"].as_str(), t["parent"].as_str(), t["agent"].as_str()), (Some("todo"), Some("SHOP-1"), Some("dev")));
        assert_eq!(update_ticket(&c, &lea, "SHOP-3", &json!({"agent": "doc"})).unwrap_err().0, 403);
        assert_eq!(hook_outbox(&c).iter().filter(|h| h.event == "ticket.approved").count(), 2);
        // a question on the sub-ticket: whoever approved the parent answers it
        update_ticket(&c, &lea, "SHOP-3", &json!({"state": "waiting"})).unwrap();
        comment(&c, &jane, "SHOP-3", &json!({"body": "Yes"})).unwrap();
        assert!(hook_outbox(&c).iter().any(|h| h.event == "ticket.answered" && h.ticket == "SHOP-3"));
        // the parent shows its progress
        kid(&lea, "SHOP-1", "todo").unwrap();
        hook_outbox(&c);
        update_ticket(&c, &jane, "SHOP-3", &json!({"state": "done"})).unwrap();
        let closed: Vec<String> = hook_outbox(&c).iter().map(|h| format!("{} {}", h.event, h.ticket)).collect();
        assert_eq!(closed, ["ticket.closed SHOP-3"]);
        let p = ticket_view(&c, &jane, "SHOP-1").unwrap();
        assert_eq!((p["progress"]["done"].as_i64(), p["progress"]["total"].as_i64(), p["children"].as_array().unwrap().len()), (Some(1), Some(2), 2));
        let listed = project(&c, &jane, "SHOP").unwrap();
        let epic = listed["tickets"].as_array().unwrap().iter().find(|k| k["id"] == "SHOP-1").unwrap().clone();
        assert_eq!(epic["progress"], json!({"total": 2, "done": 1}));
        // its text changed by someone else since the approval: covers no more
        update_ticket(&c, &who("root", true), "SHOP-1", &json!({"description": "more"})).unwrap();
        assert_eq!(kid(&lea, "SHOP-1", "todo").unwrap_err().0, 403);
        // at most MAX_CHILDREN
        for _ in 2..MAX_CHILDREN {
            kid(&jane, "SHOP-1", "backlog").unwrap();
        }
        assert_eq!(kid(&jane, "SHOP-1", "backlog").unwrap_err().0, 400);
        // cancelling the parent cancels what is still open
        update_ticket(&c, &jane, "SHOP-1", &json!({"state": "cancelled"})).unwrap();
        // a ticket without a parent closes without a call
        assert!(hook_outbox(&c).iter().all(|h| h.event != "ticket.closed"));
        let p = ticket_view(&c, &jane, "SHOP-1").unwrap();
        let states: Vec<&str> = p["children"].as_array().unwrap().iter().map(|k| k["state"].as_str().unwrap()).collect();
        assert!(states.iter().all(|s| *s == "done" || *s == "cancelled") && states.contains(&"done"), "{states:?}");
    }

    #[test]
    fn a_project_is_invisible_to_outsiders() {
        let c = setup();
        let eve = who("eve", false);
        assert_eq!(projects(&c, &eve).unwrap(), json!([]));
        assert_eq!(project(&c, &eve, "SHOP").unwrap_err().0, 404);
        assert_eq!(ticket_view(&c, &eve, "SHOP-1").unwrap_err().0, 404);
        assert_eq!(create_project(&c, &eve, &json!({"key": "EVE", "name": "x"})).unwrap_err().0, 403);
        assert_eq!(projects(&c, &who("lea", false)).unwrap()[0]["role"], "contributor");
    }

    #[test]
    fn only_an_owner_approves_and_approved_text_is_frozen() {
        let c = setup();
        let (jane, lea, tom) = (who("jane", true), who("lea", false), who("tom", false));
        // a reader comments but does not create
        assert_eq!(create_ticket(&c, &tom, "SHOP", &json!({"title": "x"})).unwrap_err().0, 403);
        // a contributor creates in backlog only
        assert_eq!(create_ticket(&c, &lea, "SHOP", &json!({"title": "x", "state": "todo"})).unwrap_err().0, 403);
        assert_eq!(create_ticket(&c, &lea, "SHOP", &json!({"title": "Idea"})).unwrap()["id"], "SHOP-1");
        comment(&c, &tom, "SHOP-1", &json!({"body": "nice"})).unwrap();
        // she edits her idea, but cannot approve it, directly or by skipping "todo"
        update_ticket(&c, &lea, "SHOP-1", &json!({"description": "v1", "priority": "high"})).unwrap();
        for state in ["todo", "in_progress", "done"] {
            assert_eq!(update_ticket(&c, &lea, "SHOP-1", &json!({"state": state})).unwrap_err().0, 403);
        }
        update_ticket(&c, &jane, "SHOP-1", &json!({"state": "todo"})).unwrap();
        // approved: the text is the owner's, the rest stays open
        assert_eq!(update_ticket(&c, &lea, "SHOP-1", &json!({"description": "v2"})).unwrap_err().0, 403);
        update_ticket(&c, &lea, "SHOP-1", &json!({"state": "in_progress", "assignee": "lea"})).unwrap();
        // back to backlog, edited: approval is needed again
        update_ticket(&c, &lea, "SHOP-1", &json!({"state": "backlog"})).unwrap();
        update_ticket(&c, &lea, "SHOP-1", &json!({"description": "v2"})).unwrap();
        assert_eq!(update_ticket(&c, &lea, "SHOP-1", &json!({"state": "todo"})).unwrap_err().0, 403);
        let t = ticket_view(&c, &tom, "SHOP-1").unwrap();
        assert_eq!((t["state"].as_str(), t["description"].as_str()), (Some("backlog"), Some("v2")));
        // history: who approved
        let approval = t["events"].as_array().unwrap().iter().find(|e| e["field"] == "state" && e["new"] == "todo").unwrap();
        assert_eq!(approval["actor"], "jane");
        assert_eq!(t["comments"][0]["author"], "tom");
    }

    #[test]
    fn projects_are_filed_under_spaces() {
        let c = setup();
        let (jane, lea) = (who("jane", true), who("lea", false));
        create_project(&c, &jane, &json!({"key": "BLOG", "name": "Blog", "space": "Initech"})).unwrap();
        create_project(&c, &jane, &json!({"key": "NAS", "name": "Cairn NAS", "space": "Globex"})).unwrap();
        // filed projects first, by space, then the unfiled ones
        let keys: Vec<String> = projects(&c, &jane).unwrap().as_array().unwrap().iter().map(|p| p["key"].as_str().unwrap().to_string()).collect();
        assert_eq!(keys, ["NAS", "BLOG", "SHOP"]);
        // an owner renames, only an administrator files
        set_member(&c, &jane, "SHOP", &json!({"login": "lea", "role": "owner"})).unwrap();
        update_project(&c, &lea, "SHOP", &json!({"name": "Shop 2", "space": ""})).unwrap();
        assert_eq!(update_project(&c, &lea, "SHOP", &json!({"space": "Globex"})).unwrap_err().0, 403);
        update_project(&c, &jane, "SHOP", &json!({"space": "Initech"})).unwrap();
        let p = project(&c, &lea, "SHOP").unwrap();
        assert_eq!((p["name"].as_str(), p["space"].as_str()), (Some("Shop 2"), Some("Initech")));
        assert_eq!(update_project(&c, &who("tom", false), "SHOP", &json!({"name": "x"})).unwrap_err().0, 403);
    }

    #[test]
    fn a_contributor_asks_an_owner_who_finds_it_first() {
        let c = setup();
        let (jane, lea) = (who("jane", true), who("lea", false));
        // a question waits on an owner, never on nobody nor on a contributor
        for v in [json!({"title": "x", "state": "waiting"}), json!({"title": "x", "state": "waiting", "assignee": "lea"})] {
            assert_eq!(create_ticket(&c, &lea, "SHOP", &v).unwrap_err().0, 403);
        }
        let q = create_ticket(&c, &lea, "SHOP", &json!({"title": "Key?", "state": "waiting", "assignee": "jane"})).unwrap();
        assert_eq!(q["id"], "SHOP-1");
        assert_eq!(notifications(&c, &jane).unwrap()["unread"], 1);
        // what is assigned to the owner, open only, the question first
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Task", "state": "todo", "assignee": "jane"})).unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Old", "state": "done", "assignee": "jane"})).unwrap();
        let list = mine(&c, &jane).unwrap();
        let ids: Vec<&str> = list.as_array().unwrap().iter().map(|k| k["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["SHOP-1", "SHOP-2"]);
        assert_eq!((list[0]["author"].as_str(), list[0]["project_name"].as_str()), (Some("lea"), Some("Shop")));
        // the owner approves: it becomes work again, nothing waits on them
        update_ticket(&c, &jane, "SHOP-1", &json!({"state": "todo"})).unwrap();
        assert_eq!(mine(&c, &lea).unwrap(), json!([]));
    }

    #[test]
    fn a_question_on_an_approved_ticket_waits_on_who_approved_it() {
        let c = setup();
        let (jane, lea) = (who("jane", true), who("lea", false));
        let unread = |a: &Actor| notifications(&c, a).unwrap()["unread"].as_i64().unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Build", "state": "todo"})).unwrap();
        update_ticket(&c, &lea, "SHOP-1", &json!({"state": "in_progress", "assignee": "lea"})).unwrap();
        // work that starts: nothing waits on the owner, and they are not told
        assert_eq!((mine(&c, &jane).unwrap(), unread(&jane)), (json!([]), 0));
        comment(&c, &lea, "SHOP-1", &json!({"body": "Which port?"})).unwrap();
        update_ticket(&c, &lea, "SHOP-1", &json!({"state": "waiting"})).unwrap();
        let list = mine(&c, &jane).unwrap();
        assert_eq!(
            (list[0]["id"].as_str(), list[0]["assignee"].as_str(), list[0]["last"]["body"].as_str()),
            (Some("SHOP-1"), Some("lea"), Some("Which port?"))
        );
        assert_eq!(unread(&jane), 2);
        // answered: it waits on whoever asked again
        comment(&c, &jane, "SHOP-1", &json!({"body": "443"})).unwrap();
        assert_eq!(mine(&c, &jane).unwrap(), json!([]));
        assert_eq!(mine(&c, &lea).unwrap()[0]["last"]["author"], "jane");
    }

    #[test]
    fn the_author_of_a_question_closes_it() {
        let c = setup();
        let (jane, lea) = (who("jane", true), who("lea", false));
        let q = |a: &Actor, t: &str| create_ticket(&c, a, "SHOP", &json!({"title": t, "state": "waiting", "assignee": "jane"})).unwrap();
        let id = q(&lea, "Key?")["id"].as_str().unwrap().to_string();
        // another author is refused, the author is allowed
        assert_eq!(close_question(&c, &jane, &id, "no").unwrap_err().0, 403);
        close_question(&c, &lea, &id, "Done without you.").unwrap();
        let v = ticket_view(&c, &lea, &id).unwrap();
        assert_eq!((v["state"].as_str(), v["comments"][0]["body"].as_str()), (Some("done"), Some("Done without you.")));
        // no longer waiting: refused (409), also after the owner moved it on
        assert_eq!(close_question(&c, &lea, &id, "again").unwrap_err().0, 409);
        // a ticket that was not opened as a question is refused, even if it waits now
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Build", "state": "todo"})).unwrap();
        update_ticket(&c, &lea, "SHOP-2", &json!({"state": "in_progress", "assignee": "lea"})).unwrap();
        update_ticket(&c, &lea, "SHOP-2", &json!({"state": "waiting"})).unwrap();
        assert_eq!(close_question(&c, &lea, "SHOP-2", "x").unwrap_err().0, 403);
        // a reader sees the question but cannot close it
        let id3 = q(&lea, "Port?")["id"].as_str().unwrap().to_string();
        assert!(close_question(&c, &who("tom", false), &id3, "x").is_err());
        assert_eq!(ticket_view(&c, &lea, &id3).unwrap()["state"], "waiting");
    }

    #[test]
    fn the_database_and_a_project_are_copied_out() {
        let c = setup();
        let (jane, tom) = (who("jane", true), who("tom", false));
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Build", "description": "All of it"})).unwrap();
        comment(&c, &jane, "SHOP-1", &json!({"body": "Go"})).unwrap();
        // a project in full, for any member; nothing for an outsider
        let out = export_project(&c, &tom, "SHOP").unwrap();
        assert_eq!((out["tickets"][0]["description"].as_str(), out["tickets"][0]["comments"][0]["body"].as_str()), (Some("All of it"), Some("Go")));
        assert_eq!(export_project(&c, &who("eve", false), "SHOP").unwrap_err().0, 404);
        // a copy of the database that opens on its own; the oldest copies go
        let dir = std::env::temp_dir().join(format!("cairn-plan-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in 0..COPIES + 2 {
            std::fs::write(dir.join(format!("plan-00000000{n:02}.db")), "old").unwrap();
        }
        let path = backup(&c, dir.to_str().unwrap()).unwrap();
        let copy = Connection::open(&path).unwrap();
        assert_eq!(copy.query_row("SELECT COUNT(*) FROM tickets", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        let left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left.len(), COPIES);
        assert!(!left.contains(&"plan-0000000000.db".to_string()) && left.contains(&"plan-0000000015.db".to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tickets_are_searched_in_the_projects_someone_sees() {
        let c = setup();
        let (jane, lea, eve) = (who("jane", true), who("lea", false), who("eve", false));
        create_project(&c, &jane, &json!({"key": "BLOG", "name": "Blog"})).unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Port of the resolver", "description": "It listens on 853."})).unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Other"})).unwrap();
        comment(&c, &lea, "SHOP-2", &json!({"body": "We chose the PORT 8443 for 100% of the calls."})).unwrap();
        create_ticket(&c, &jane, "BLOG", &json!({"title": "Port the app"})).unwrap();
        let ids = |a: &Actor, q: &str, key: &str| -> Vec<String> {
            let mut ids: Vec<String> = search(&c, a, q, key).unwrap().as_array().unwrap().iter().map(|k| k["id"].as_str().unwrap().to_string()).collect();
            ids.sort();
            ids
        };
        // title, description and comments; a member sees their projects only, an outsider nothing
        assert_eq!(ids(&lea, "port", ""), ["SHOP-1", "SHOP-2"]);
        assert_eq!(ids(&jane, "port", ""), ["BLOG-1", "SHOP-1", "SHOP-2"]);
        assert_eq!(ids(&jane, "port", "BLOG"), ["BLOG-1"]);
        assert_eq!(ids(&lea, "853", ""), ["SHOP-1"]);
        assert!(ids(&eve, "port", "").is_empty());
        // the id of a ticket, and the signs of LIKE read as themselves
        assert_eq!(ids(&lea, "shop-2", ""), ["SHOP-2"]);
        assert_eq!(ids(&lea, "100%", ""), ["SHOP-2"]);
        assert!(ids(&lea, "1_0", "").is_empty());
        let found = search(&c, &lea, "8443", "").unwrap();
        assert!(found[0]["excerpt"].as_str().unwrap().contains("PORT 8443"));
        assert_eq!(search(&c, &lea, "p", "").unwrap_err().0, 400);
    }

    #[test]
    fn a_ticket_sleeps_until_its_date() {
        let c = setup();
        let jane = who("jane", true);
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Later", "state": "todo", "assignee": "jane", "wake_date": "2999-01-01"})).unwrap();
        // asleep: not in "for me", not to be taken
        assert_eq!(mine(&c, &jane).unwrap(), json!([]));
        assert!(asleep(&c, &jane, "SHOP-1").unwrap());
        hook_outbox(&c);
        assert_eq!(wake(&c), 0);
        // its date has come: it is back, the date is cleared, an agent is called, the phone rings
        update_ticket(&c, &jane, "SHOP-1", &json!({"wake_date": "2000-01-01"})).unwrap();
        assert_eq!(wake(&c), 1);
        assert!(!asleep(&c, &jane, "SHOP-1").unwrap());
        assert_eq!(mine(&c, &jane).unwrap()[0]["id"], "SHOP-1");
        assert_eq!(ticket_view(&c, &jane, "SHOP-1").unwrap()["wake_date"], "");
        assert!(hook_outbox(&c).iter().any(|h| h.event == "ticket.approved" && h.ticket == "SHOP-1"));
        assert!(outbox(&c).iter().any(|n| n.kind == "wake" && n.question));
        assert_eq!(update_ticket(&c, &jane, "SHOP-1", &json!({"wake_date": "soon"})).unwrap_err().0, 400);
    }

    #[test]
    fn a_draft_is_imported_once() {
        assert_eq!(epoch("1970-01-02"), Some(86_400 + 43_200));
        assert_eq!(epoch("2000-03-01"), Some(951_868_800 + 43_200));
        let c = open(":memory:");
        let draft = json!({
            "projet": "Shop", "depot": "https://git.example/shop", "depot_infra": "https://git.example/iac",
            "description": "Shop.", "membres": {"claude": "contributor"},
            "familles": {"M": "Phase 1"},
            "lots": [
                {"titre": "Second", "jalon": "M2", "debut": "2026-10-02", "fin": "2026-10-03", "etat": "en_cours",
                 "objectif": "B", "choix": ["x"], "ecarte": [], "resultat": "",
                 "sources": ["PR 12", "commit abcdef1", "docs/a.md", "dépôt infra : docs/12-shop.md", "un fichier.docx"]},
                {"titre": "Premier", "jalon": "", "debut": "2026-09-10", "fin": "2026-09-10", "etat": "termine",
                 "objectif": "A", "choix": [], "ecarte": [], "resultat": "R", "sources": []}
            ],
            "decisions": [{"titre": "Nom", "date": "2026-09-10", "resume": "Shop", "source": "docs/cadrage.md"}],
            "incertitudes": ["?"]
        });
        let jane = who("jane", false);
        assert!(import(&c, "jane", "Initech", &draft).unwrap().contains("4 tickets created"));
        // a replay creates nothing twice and leaves what a person changed
        update_ticket(&c, &jane, "SHOP-1", &json!({"priority": "high", "state": "cancelled"})).unwrap();
        assert!(import(&c, "jane", "Initech", &draft).unwrap().contains("0 tickets created, 4 brought in line, 0 links"));
        let p = project(&c, &jane, "SHOP").unwrap();
        assert_eq!((p["space"].as_str(), p["role"].as_str(), p["tickets"].as_array().unwrap().len()), (Some("Initech"), Some("owner"), 4));
        assert_eq!((p["description"].as_str(), p["members"][0]["login"].as_str(), p["members"][0]["role"].as_str()), (Some("Shop."), Some("claude"), Some("contributor")));
        assert!(import(&c, "jane", "", &json!({"projet": "X1", "membres": {"eve": "boss"}})).is_err());
        // the oldest lot comes first and is dated by its start
        let first = ticket_view(&c, &jane, "SHOP-1").unwrap();
        assert_eq!((first["title"].as_str(), first["state"].as_str(), first["priority"].as_str()), (Some("Premier"), Some("cancelled"), Some("high")));
        assert_eq!(first["created"], epoch("2026-09-10").unwrap());
        assert_eq!(first["assignee"], "jane");
        let second = ticket_view(&c, &jane, "SHOP-2").unwrap();
        assert_eq!((second["title"].as_str(), second["milestone"].as_str(), second["state"].as_str()), (Some("M2 · Second"), Some("Phase 1"), Some("in_progress")));
        assert!(second["description"].as_str().unwrap().contains("Choix et raisons\n- x"));
        let urls: Vec<&str> = second["links"].as_array().unwrap().iter().map(|l| l["url"].as_str().unwrap()).collect();
        assert_eq!(urls, [
            "https://git.example/shop/pulls/12",
            "https://git.example/shop/commit/abcdef1",
            "https://git.example/shop/src/branch/main/docs/a.md",
            "https://git.example/iac/src/branch/main/docs/12-shop.md",
        ]);
        assert_eq!(ticket_view(&c, &jane, "SHOP-3").unwrap()["milestone"], "Décisions");
        assert_eq!(ticket_view(&c, &jane, "SHOP-4").unwrap()["state"], "todo");
        // a person files a ticket under a milestone by hand
        update_ticket(&c, &jane, "SHOP-4", &json!({"milestone": "Phase 1"})).unwrap();
        assert_eq!(project(&c, &jane, "SHOP").unwrap()["tickets"][0]["milestone"], "Phase 1");
    }

    #[test]
    fn approvals_and_answers_are_announced() {
        let c = open(":memory:");
        let jane = Actor { login: "jane".into(), admin: true };
        let claude = Actor { login: "claude".into(), admin: false };
        create_project(&c, &jane, &json!({"key": "SHOP", "name": "Shop"})).unwrap();
        set_member(&c, &jane, "SHOP", &json!({"login": "claude", "role": "contributor"})).unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Idea"})).unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Approved at birth", "state": "todo"})).unwrap();
        let calls = hook_outbox(&c);
        assert_eq!((calls.len(), calls[0].event.as_str(), calls[0].ticket.as_str(), calls[0].project.as_str()), (1, "ticket.approved", "SHOP-2", "SHOP"));
        assert!(hook_outbox(&c).is_empty()); // handed over once

        // approved later; a comment on a ticket that does not wait says nothing
        update_ticket(&c, &jane, "SHOP-1", &json!({"state": "todo", "priority": "high"})).unwrap();
        comment(&c, &jane, "SHOP-1", &json!({"body": "Go."})).unwrap();
        let calls = hook_outbox(&c);
        assert_eq!((calls.len(), calls[0].ticket.as_str(), calls[0].actor.as_str(), calls[0].title.as_str()), (1, "SHOP-1", "jane", "Idea"));

        // waiting: only the answer of whoever approved it is announced
        update_ticket(&c, &claude, "SHOP-1", &json!({"state": "in_progress"})).unwrap();
        comment(&c, &claude, "SHOP-1", &json!({"body": "Which one?"})).unwrap();
        update_ticket(&c, &claude, "SHOP-1", &json!({"state": "waiting"})).unwrap();
        comment(&c, &claude, "SHOP-1", &json!({"body": "Still waiting."})).unwrap();
        assert!(hook_outbox(&c).is_empty());
        comment(&c, &jane, "SHOP-1", &json!({"body": "The first."})).unwrap();
        let calls = hook_outbox(&c);
        assert_eq!((calls.len(), calls[0].event.as_str(), calls[0].ticket.as_str()), (1, "ticket.answered", "SHOP-1"));

        assert!(save_settings(&c, &jane, &json!({"agent_hook_url": "ftp://x"})).is_err());
        assert!(save_settings(&c, &jane, &json!({"agent_hook_secret": "short"})).is_err());
        save_settings(&c, &jane, &json!({"agent_hook_url": "http://127.0.0.1:9095/hook", "agent_hook_secret": "0123456789abcdef"})).unwrap();
        assert_eq!(settings(&c)["agent_hook_url"], "http://127.0.0.1:9095/hook");
    }

    #[test]
    fn settings_are_for_administrators_and_topics_follow_the_prefix() {
        let c = setup();
        let (jane, lea) = (who("jane", true), who("lea", false));
        assert!(save_settings(&c, &lea, &json!({"smtp_host": "smtp.example.org"})).is_err());
        save_settings(&c, &jane, &json!({"smtp_host": "smtp.example.org", "smtp_port": "587", "smtp_password": "secret", "ntfy_prefix": "plan-"})).unwrap();
        // an empty secret keeps the saved one; an empty value of anything else is saved
        save_settings(&c, &jane, &json!({"smtp_password": "", "smtp_port": ""})).unwrap();
        let saved = settings(&c);
        assert_eq!((saved["smtp_password"].as_str(), saved["smtp_port"].as_str(), saved["smtp_host"].as_str()), ("secret", "", "smtp.example.org"));
        for bad in [json!({"smtp_port": "0"}), json!({"smtp_port": "smtp"}), json!({"public_url": "plan.example.org"}), json!({"ntfy_prefix": "a b"})] {
            assert!(save_settings(&c, &jane, &bad).is_err(), "{bad}");
        }
        // one's own topic: with the prefix, and nothing but letters, digits, - and _
        seen(&c, "lea", "Lea", "lea@example.org");
        assert!(set_prefs(&c, &lea, &json!({"ntfy_topic": "alerts"})).is_err());
        assert!(set_prefs(&c, &lea, &json!({"ntfy_topic": "plan-lea/x"})).is_err());
        set_prefs(&c, &lea, &json!({"ntfy_topic": "plan-lea", "mail": false})).unwrap();
        assert_eq!((ntfy_topic(&c, "lea").as_str(), wants_mail(&c, "lea")), ("plan-lea", false));
        set_prefs(&c, &lea, &json!({"ntfy_topic": ""})).unwrap();
        assert_eq!(ntfy_topic(&c, "lea"), "");
        assert!(set_prefs(&c, &lea, &json!({})).is_err());
    }

    #[test]
    fn time_is_logged_and_counted_per_state() {
        let c = setup();
        let (jane, lea, tom) = (who("jane", true), who("lea", false), who("tom", false));
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Task", "state": "todo"})).unwrap();
        update_ticket(&c, &lea, "SHOP-1", &json!({"state": "in_progress"})).unwrap();
        update_ticket(&c, &lea, "SHOP-1", &json!({"state": "waiting"})).unwrap(); // a contributor may pause
        log_work(&c, &lea, "SHOP-1", &json!({"minutes": 90, "day": "2026-11-18", "note": "Draft"})).unwrap();
        log_work(&c, &jane, "SHOP-1", &json!({"minutes": 30})).unwrap();
        assert!(log_work(&c, &tom, "SHOP-1", &json!({"minutes": 5})).is_err()); // a reader does not log
        assert!(log_work(&c, &lea, "SHOP-1", &json!({"minutes": 0})).is_err());
        assert!(log_work(&c, &lea, "SHOP-1", &json!({"minutes": 5, "day": "yesterday"})).is_err());
        let t = ticket_view(&c, &tom, "SHOP-1").unwrap();
        assert_eq!((t["time"]["logged"].as_i64(), t["work"].as_array().unwrap().len()), (Some(120), 2));
        for state in ["todo", "in_progress", "waiting"] {
            assert!(t["time"]["states"][state].is_i64(), "{state}");
        }
        // one removes one's own entry, a project owner anyone's
        let entry = |login: &str| t["work"].as_array().unwrap().iter().find(|w| w["login"] == login).unwrap()["id"].as_i64().unwrap();
        assert!(log_work(&c, &lea, "SHOP-1", &json!({"delete": entry("jane")})).is_err());
        log_work(&c, &jane, "SHOP-1", &json!({"delete": entry("lea")})).unwrap();
        assert_eq!(ticket_view(&c, &tom, "SHOP-1").unwrap()["time"]["logged"], 30);

        // the clock: taken at 100, paused at 200, resumed at 300, closed at 400
        let events = [
            json!({"field": "state", "new": "done", "at": 400}),
            json!({"field": "state", "new": "in_progress", "at": 300}),
            json!({"field": "assignee", "new": "lea", "at": 250}),
            json!({"field": "state", "new": "waiting", "at": 200}),
            json!({"field": "state", "new": "in_progress", "at": 100}),
            json!({"field": "created", "new": "todo", "at": 40}),
        ];
        assert_eq!(state_seconds(&events, &json!({}), 1000), json!({"todo": 60, "in_progress": 200, "waiting": 100, "done": 0}));
        assert_eq!(state_seconds(&events[3..], &json!({}), 1000), json!({"todo": 60, "in_progress": 100, "waiting": 800}));
    }

    #[test]
    fn time_is_reported_per_project_and_per_space() {
        let c = setup();
        let (jane, lea, eve) = (who("jane", true), who("lea", false), who("eve", false));
        let agents = vec!["claude".to_string()];
        set_member(&c, &jane, "SHOP", &json!({"login": "claude", "role": "contributor"})).unwrap();
        let claude = who("claude", false);
        update_project(&c, &jane, "SHOP", &json!({"space": "Acme"})).unwrap();
        create_project(&c, &jane, &json!({"key": "DEMO", "name": "Demo", "space": "Acme"})).unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Epic", "state": "todo", "milestone": "M1"})).unwrap();
        create_ticket(&c, &jane, "SHOP", &json!({"title": "Part", "parent": "SHOP-1"})).unwrap();
        log_work(&c, &lea, "SHOP-1", &json!({"minutes": 90, "day": "2026-11-18"})).unwrap();
        log_work(&c, &claude, "SHOP-2", &json!({"minutes": 7})).unwrap();
        comment(&c, &claude, "SHOP-2", &json!({"body": "Done: PR 3.\n\ndev · some-model · 0.46 $"})).unwrap();
        comment(&c, &claude, "SHOP-2", &json!({"body": "Second run.\n\ndev · some-model · 0.10 $\n"})).unwrap();
        comment(&c, &lea, "SHOP-2", &json!({"body": "I paid · 9 $"})).unwrap(); // not an agent: no cost
        update_ticket(&c, &jane, "SHOP-1", &json!({"state": "done"})).unwrap();

        let p = time_report(&c, &lea, Scope::Project("SHOP"), &agents).unwrap();
        assert_eq!((p["scope"].as_str(), p["space"].as_str(), p["tickets"].as_array().unwrap().len()), (Some("project"), Some("Acme"), 2));
        let t1 = &p["tickets"][0];
        assert_eq!((t1["id"].as_str(), t1["milestone"].as_str(), t1["state"].as_str()), (Some("SHOP-1"), Some("M1"), Some("done")));
        assert!(t1["closed"].as_i64().unwrap() > 0 && t1["states"]["todo"].is_i64());
        let t2 = &p["tickets"][1];
        assert_eq!((t2["parent"].as_str(), t2["closed"].as_i64(), t2["usd"].as_f64()), (Some("SHOP-1"), Some(0), Some(0.56)));
        let minutes: Vec<i64> = p["work"].as_array().unwrap().iter().map(|w| w["minutes"].as_i64().unwrap()).collect();
        assert_eq!(minutes, vec![90, 7]);
        assert_eq!(p["agents"], json!(["claude"]));
        let costs: Vec<(&str, f64)> = p["costs"].as_array().unwrap().iter().map(|k| (k["login"].as_str().unwrap(), k["usd"].as_f64().unwrap())).collect();
        assert_eq!(costs, vec![("claude", 0.46), ("claude", 0.10)]);

        // the space: every project of it the viewer sees, only those
        let s = time_report(&c, &jane, Scope::Space("Acme"), &agents).unwrap();
        assert_eq!(s["projects"].as_array().unwrap().len(), 2);
        assert_eq!(time_report(&c, &lea, Scope::Space("Acme"), &agents).unwrap()["projects"], json!([{"key": "SHOP", "name": "Shop"}]));
        assert!(time_report(&c, &eve, Scope::Space("Acme"), &agents).is_err());
        assert!(time_report(&c, &eve, Scope::Project("SHOP"), &agents).is_err());
        assert_eq!((reported_usd("x\n\ndoc · m · 0.08 $"), reported_usd("costs 3 $"), reported_usd("a · b · -1 $")), (Some(0.08), None, None));

        // hourly rates: set by an administrator, shown to an owner of the project, not to a contributor
        seen(&c, "lea", "Lea", "");
        assert!(set_rate(&c, &lea, &json!({"login": "lea", "rate": 6500})).is_err());
        assert!(set_rate(&c, &jane, &json!({"login": "lea", "rate": -1})).is_err());
        assert!(set_rate(&c, &jane, &json!({"login": "nobody", "rate": 10})).is_err());
        set_rate(&c, &jane, &json!({"login": "lea", "rate": 6500, "currency": "CHF"})).unwrap();
        let p = time_report(&c, &jane, Scope::Project("SHOP"), &agents).unwrap();
        assert_eq!((p["money"].as_bool(), p["rates"]["lea"].as_i64(), p["currency"].as_str()), (Some(true), Some(6500), Some("CHF")));
        let p = time_report(&c, &lea, Scope::Project("SHOP"), &agents).unwrap();
        assert_eq!((p["money"].as_bool(), p["rates"].as_object().unwrap().len()), (Some(false), 0));
        set_member(&c, &jane, "SHOP", &json!({"login": "lea", "role": "owner"})).unwrap();
        assert_eq!(time_report(&c, &lea, Scope::Project("SHOP"), &agents).unwrap()["rates"]["lea"], 6500);
        assert_eq!(time_report(&c, &lea, Scope::Space("Acme"), &agents).unwrap()["money"], true); // owner of all she sees
    }

    #[test]
    fn a_photo_is_set_by_its_owner_or_by_an_administrator_for_an_agent() {
        let c = setup();
        let (jane, lea) = (who("jane", true), who("lea", false));
        let agents = vec!["claude".to_string()];
        seen(&c, "lea", "Lea", "lea@example.org");
        assert_eq!(photos(&c).unwrap(), json!({}));
        let v = set_photo(&c, &lea, "lea", &agents, true).unwrap();
        assert!(v > 0);
        assert_eq!(photos(&c).unwrap(), json!({ "lea": v }));
        // nobody chooses the photo of someone else, not even an administrator
        assert_eq!(set_photo(&c, &lea, "tom", &agents, true).unwrap_err().0, 403);
        assert_eq!(set_photo(&c, &jane, "lea", &agents, true).unwrap_err().0, 403);
        // removing it: an administrator only
        assert_eq!(set_photo(&c, &lea, "jane", &agents, false).unwrap_err().0, 403);
        assert_eq!(set_photo(&c, &jane, "lea", &agents, false).unwrap(), 0);
        assert_eq!(photos(&c).unwrap(), json!({}));
        let v = set_photo(&c, &lea, "lea", &agents, true).unwrap();
        assert_eq!(photos(&c).unwrap(), json!({ "lea": v }));
        // an agent: an administrator only, even before the agent first called
        assert_eq!(set_photo(&c, &lea, "claude", &agents, true).unwrap_err().0, 403);
        assert!(set_photo(&c, &jane, "claude", &agents, true).unwrap() > 0);
        assert_eq!(photos(&c).unwrap().as_object().unwrap().len(), 2);
        assert_eq!(set_photo(&c, &jane, "../x", &agents, true).unwrap_err().0, 400);
        // removed: initials again; what is known of the person stays
        assert_eq!(set_photo(&c, &lea, "lea", &agents, false).unwrap(), 0);
        assert_eq!(photos(&c).unwrap().as_object().unwrap().keys().collect::<Vec<_>>(), ["claude"]);
        assert!(wants_mail(&c, "lea"));
    }

    #[test]
    fn files_follow_the_rights_of_the_ticket() {
        let c = setup();
        let (lea, tom, eve) = (who("lea", false), who("tom", false), who("eve", false));
        create_ticket(&c, &lea, "SHOP", &json!({"title": "Idea"})).unwrap();
        let fid = add_file(&c, &lea, "SHOP-1", "..\\..\\etc/Devis été.pdf", 1200).unwrap();
        assert_eq!(file(&c, &tom, "SHOP-1", fid).unwrap(), "Devis été.pdf"); // a reader reads; the name lost its path
        assert_eq!(ticket_view(&c, &tom, "SHOP-1").unwrap()["files"][0]["size"], 1200);
        assert!(add_file(&c, &tom, "SHOP-1", "x.txt", 1).is_err()); // but does not attach
        assert!(file(&c, &eve, "SHOP-1", fid).is_err()); // and an outsider sees nothing
        assert!(add_file(&c, &lea, "SHOP-1", "/", 1).is_err());
        assert!(delete_file(&c, &tom, "SHOP-1", &json!({"delete": fid})).is_err());
        assert_eq!(delete_file(&c, &lea, "SHOP-1", &json!({"delete": fid})).unwrap(), fid);
        assert!(file(&c, &lea, "SHOP-1", fid).is_err());
    }

    #[test]
    fn participants_are_told() {
        let c = setup();
        let (jane, lea, tom) = (who("jane", true), who("lea", false), who("tom", false));
        let unread = |a: &Actor| notifications(&c, a).unwrap()["unread"].as_i64().unwrap();
        create_ticket(&c, &lea, "SHOP", &json!({"title": "Idea"})).unwrap();
        comment(&c, &jane, "SHOP-1", &json!({"body": "Why?"})).unwrap();
        update_ticket(&c, &jane, "SHOP-1", &json!({"state": "todo", "assignee": "tom"})).unwrap();
        // lea, the author: the comment and the approval; tom: the assignment; jane did it all
        assert_eq!((unread(&lea), unread(&tom), unread(&jane)), (2, 1, 0));
        comment(&c, &tom, "SHOP-1", &json!({"body": "On it."})).unwrap();
        assert_eq!((unread(&lea), unread(&tom), unread(&jane)), (3, 1, 1));
        let first = &notifications(&c, &lea).unwrap()["items"][0];
        assert_eq!((first["ticket"].as_str(), first["kind"].as_str(), first["detail"].as_str()), (Some("SHOP-1"), Some("comment"), Some("On it.")));
        // handed over once to the sender, with the address the proxy gave, even if a later request gave none
        seen(&c, "lea", "Lea", "lea@example.org");
        seen(&c, "lea", "", "");
        let waiting = outbox(&c);
        assert!(waiting.iter().any(|n| n.login == "lea" && n.email == "lea@example.org"));
        assert_eq!((waiting.len(), outbox(&c).len()), (5, 0));
        read_notifications(&c, &lea, &json!({"ticket": "SHOP-1"})).unwrap();
        assert_eq!(unread(&lea), 0);
        // someone removed from the project is not told any more
        set_member(&c, &jane, "SHOP", &json!({"login": "lea", "role": null})).unwrap();
        comment(&c, &jane, "SHOP-1", &json!({"body": "Done."})).unwrap();
        assert_eq!((unread(&lea), unread(&tom)), (0, 2));
        assert!(read_notifications(&c, &tom, &json!({})).is_err());
    }

    #[test]
    fn input_is_checked() {
        let c = setup();
        let jane = who("jane", true);
        create_ticket(&c, &jane, "SHOP", &json!({"title": "T", "start_date": "2026-11-18"})).unwrap();
        for v in [
            json!({"title": ""}),
            json!({"state": "approved"}),
            json!({"priority": "max"}),
            json!({"assignee": "eve"}),
            json!({"end_date": "18/11/2026"}),
            json!({"end_date": "2026-13-01"}),
        ] {
            assert_eq!(update_ticket(&c, &jane, "SHOP-1", &v).unwrap_err().0, 400, "{v}");
        }
        assert_eq!(link(&c, &jane, "SHOP-1", &json!({"url": "javascript:alert(1)"})).unwrap_err().0, 400);
        link(&c, &jane, "SHOP-1", &json!({"url": "https://git.example/pulls/3", "title": "PR 3"})).unwrap();
        let id = ticket_view(&c, &jane, "SHOP-1").unwrap()["links"][0]["id"].as_i64().unwrap();
        link(&c, &jane, "SHOP-1", &json!({"delete": id})).unwrap();
        assert_eq!(create_project(&c, &jane, &json!({"key": "shop", "name": "x"})).unwrap_err().0, 400);
        assert_eq!(create_project(&c, &jane, &json!({"key": "SHOP", "name": "x"})).unwrap_err().0, 409);
        assert_eq!(set_member(&c, &jane, "SHOP", &json!({"login": "Bad Login", "role": "reader"})).unwrap_err().0, 400);
        // removing a member frees their tickets
        update_ticket(&c, &jane, "SHOP-1", &json!({"assignee": "lea"})).unwrap();
        set_member(&c, &jane, "SHOP", &json!({"login": "lea", "role": null})).unwrap();
        assert_eq!(ticket_view(&c, &jane, "SHOP-1").unwrap()["assignee"], "");
    }
    fn run(id: &str) -> Value {
        json!({"run_id": id, "agent": "ouvrier", "source": "cairn-plan-agent", "task": "Fix the header", "state": "running", "started": 1000,
               "ticket": "SHOP-1", "model": "claude-sonnet-5-5"})
    }

    #[test]
    fn a_run_is_reported_then_updated_by_the_same_id() {
        let c = setup();
        let lea = who("lea", false);
        let first = report_run(&c, &lea, &run("r1")).unwrap();
        // the project is read from the ticket, an end comes with the end state
        assert_eq!((first["project"].as_str(), first["state"].as_str(), first["ended"].clone(), first["login"].as_str()), (Some("SHOP"), Some("running"), Value::Null, Some("lea")));
        let end = report_run(&c, &lea, &json!({"run_id": "r1", "state": "ok", "ended": 1090, "cost_usd": 0.46, "tokens_out": 3000, "result": "conforme"})).unwrap();
        // what the second report leaves out is kept; nothing was added twice
        assert_eq!((end["state"].as_str(), end["task"].as_str(), end["model"].as_str(), end["started"].as_i64()), (Some("ok"), Some("Fix the header"), Some("claude-sonnet-5-5"), Some(1000)));
        assert_eq!((end["ended"].as_i64(), end["cost_usd"].as_f64(), end["tokens_in"].clone(), end["result"].as_str()), (Some(1090), Some(0.46), Value::Null, Some("conforme")));
        assert_eq!(runs(&c, &lea, "", "", "").unwrap().as_array().unwrap().len(), 1);
        // an end state without an end date gets one
        // sent again as running (a retry): it is going on, the old end is dropped
        let again = report_run(&c, &lea, &json!({"run_id": "r1", "state": "running"})).unwrap();
        assert_eq!(again["ended"], Value::Null);
        let late = report_run(&c, &lea, &json!({"run_id": "r2", "agent": "controleur", "source": "claude-code", "task": "Check", "state": "failed", "started": 1100})).unwrap();
        assert!(late["ended"].as_i64().unwrap() >= 1100);
    }

    #[test]
    fn an_account_reports_its_own_runs_only() {
        let c = setup();
        let (lea, tom, eve) = (who("lea", false), who("tom", false), who("eve", false));
        report_run(&c, &lea, &run("r1")).unwrap();
        // another account, even a member or an administrator, cannot write that run
        for other in [&tom, &eve, &who("jane", true)] {
            let r = report_run(&c, other, &json!({"run_id": "r1", "state": "ok", "ended": 2000}));
            assert_eq!(r.unwrap_err().0, 403, "{}", other.login);
        }
        assert_eq!(runs(&c, &lea, "", "", "").unwrap()[0]["state"], "running");
        // a reader or an outsider cannot report for the project either
        assert_eq!(report_run(&c, &tom, &run("r3")).unwrap_err().0, 403);
        assert_eq!(report_run(&c, &eve, &run("r4")).unwrap_err().0, 404);
    }

    #[test]
    fn runs_are_seen_like_the_tickets_of_their_project() {
        let c = setup();
        let (jane, lea, tom, eve) = (who("jane", true), who("lea", false), who("tom", false), who("eve", false));
        report_run(&c, &lea, &run("of-project")).unwrap();
        let mut free = run("no-project");
        free["ticket"] = json!("");
        report_run(&c, &lea, &free).unwrap();
        let ids = |a: &Actor| -> Vec<String> {
            runs(&c, a, "", "", "").unwrap().as_array().unwrap().iter().map(|r| r["run_id"].as_str().unwrap().to_string()).collect()
        };
        // a member (any role) sees the run of the project; only an administrator sees the run without one
        assert_eq!(ids(&lea), ["of-project"]);
        assert_eq!(ids(&tom), ["of-project"]);
        assert!(ids(&eve).is_empty());
        assert_eq!(ids(&jane).len(), 2);
        // filters: project, agent, date
        assert_eq!(runs(&c, &jane, "", "", "SHOP").unwrap().as_array().unwrap().len(), 1);
        assert!(runs(&c, &jane, "", "controleur", "").unwrap().as_array().unwrap().is_empty());
        assert_eq!(runs(&c, &jane, "", "ouvrier", "").unwrap().as_array().unwrap().len(), 2);
        let far = now() - 10;
        report_run(&c, &lea, &json!({"run_id": "of-project", "state": "ok", "ended": far})).unwrap();
        assert!(runs(&c, &jane, "2099-01-01", "", "").unwrap().as_array().unwrap().is_empty());
        assert_eq!(runs(&c, &jane, "2026-01-01", "", "").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(runs(&c, &jane, &(far + 5).to_string(), "", "").unwrap().as_array().unwrap().len(), 1); // only the one still going on
        assert_eq!(runs(&c, &jane, "yesterday", "", "").unwrap_err().0, 400);
    }

    #[test]
    fn a_run_is_checked() {
        let c = setup();
        let lea = who("lea", false);
        let with = |k: &str, v: Value| {
            let mut r = run("r1");
            r[k] = v;
            report_run(&c, &lea, &r).unwrap_err().0
        };
        assert_eq!(with("state", json!("done")), 400);
        assert_eq!(with("source", json!("My Script")), 400);
        assert_eq!(with("task", json!("x".repeat(121))), 400);
        assert_eq!(with("task", json!("")), 400);
        assert_eq!(with("task", json!("two\nlines")), 400);
        assert_eq!(with("ticket", json!("shop-1")), 400);
        assert_eq!(with("project", json!("BLOG")), 400); // the ticket is SHOP-1
        assert_eq!(with("cost_usd", json!(-1)), 400);
        assert_eq!(with("tokens_in", json!(1.5)), 400);
        assert_eq!(with("started", json!(0)), 400);
        assert_eq!(with("ended", json!(5)), 400);
        assert_eq!(report_run(&c, &lea, &json!({"run_id": "", "agent": "a"})).unwrap_err().0, 400);
        assert_eq!(report_run(&c, &lea, &json!({"run_id": "x".repeat(101)})).unwrap_err().0, 400);
        assert!(runs(&c, &who("jane", true), "", "", "").unwrap().as_array().unwrap().is_empty());
        // a task of exactly 120 characters is fine
        let mut r = run("r2");
        r["task"] = json!("é".repeat(120));
        report_run(&c, &lea, &r).unwrap();
    }
}

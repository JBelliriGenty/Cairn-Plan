//! Webhooks of a Git forge: a pull request or a commit that names a ticket
//! ("SHOP-51" in its title, text, branch or message) is attached to it.
//! And the webhook that leaves (`send`): a ticket an agent may act on,
//! signed the way the forges sign theirs.
//!
//! Forgejo, Gitea and GitHub send the same fields and sign the body the same
//! way (HMAC-SHA256 of the raw body, header X-Hub-Signature-256), so one
//! endpoint serves them all. The forge is no member: it can only add links,
//! and only to tickets that exist.

use crate::store;
use hmac::{Hmac, Mac};
use rusqlite::Connection;
use serde_json::Value;
use sha2::Sha256;

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// True if `header` ("sha256=<hex>") is the signature of `body` under `secret`.
pub fn signed(secret: &str, body: &[u8], header: &str) -> bool {
    let Some(sig) = header.strip_prefix("sha256=").and_then(unhex) else { return false };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(body);
    mac.verify_slice(&sig).is_ok() // constant time
}

/// The header value that signs `body` under `secret`: "sha256=<hex>".
pub fn signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(body);
    let hex: String = mac.finalize().into_bytes().iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256={hex}")
}

/// The body of the outgoing webhook: which ticket, what happened, who did it.
/// The text of the ticket is not in it: the receiver reads the ticket itself.
pub fn body(call: &store::Call) -> String {
    serde_json::json!({
        "event": call.event, "ticket": call.ticket, "project": call.project, "title": call.title, "by": call.actor, "at": call.at,
    })
    .to_string()
}

/// Posts one call to `url`, signed with `secret` (header X-Hub-Signature-256).
pub fn send(url: &str, secret: &str, call: &store::Call) -> Result<(), String> {
    if secret.is_empty() {
        return Err("no secret is set for the agent webhook".into());
    }
    let body = body(call);
    ureq::post(url)
        .timeout(std::time::Duration::from_secs(10))
        .set("Content-Type", "application/json")
        .set("X-Cairn-Event", &call.event)
        .set("X-Hub-Signature-256", &signature(secret, body.as_bytes()))
        .send_string(&body)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Ticket ids named in a text: "SHOP-51", also inside "SHOP-51-fix".
/// ponytail: "UTF-8" is read as an id too; harmless, no such ticket exists.
pub fn ticket_ids(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-')) {
        let parts: Vec<&str> = token.split('-').collect();
        for w in parts.windows(2) {
            if store::valid_key(w[0]) && (1..=9).contains(&w[1].len()) && w[1].bytes().all(|b| b.is_ascii_digit()) {
                let id = format!("{}-{}", w[0], w[1]);
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    out
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// What a payload is about: (address, label, text to search for ticket ids).
fn subjects(p: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let pr = &p["pull_request"];
    if pr.is_object() {
        let status = if pr["merged"] == true {
            " (merged)"
        } else if pr["state"] == "closed" {
            " (closed)"
        } else {
            ""
        };
        out.push((
            s(&pr["html_url"]).to_string(),
            format!("PR {}: {}{status}", pr["number"], s(&pr["title"])),
            format!("{}\n{}\n{}", s(&pr["title"]), s(&pr["body"]), s(&pr["head"]["ref"])),
        ));
    }
    for c in p["commits"].as_array().into_iter().flatten() {
        let (id, message) = (s(&c["id"]), s(&c["message"]));
        out.push((
            s(&c["url"]).to_string(),
            format!("Commit {}: {}", id.get(..7).unwrap_or(id), message.lines().next().unwrap_or("")),
            message.to_string(),
        ));
    }
    out
}

/// Attaches what the payload is about to the tickets it names. Returns their ids.
pub fn handle(c: &Connection, payload: &Value) -> Vec<String> {
    let mut linked = Vec::new();
    for (url, title, text) in subjects(payload) {
        for id in ticket_ids(&text) {
            if store::hook_link(c, &id, &url, &title).unwrap_or(false) && !linked.contains(&id) {
                linked.push(id);
            }
        }
    }
    linked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Actor;
    use serde_json::json;

    #[test]
    fn the_signature_is_checked() {
        // RFC 4231, test case 2
        let (key, data) = ("Jefe", b"what do ya want for nothing?");
        let good = "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843";
        assert!(signed(key, data, good));
        assert!(!signed("other", data, good));
        assert!(!signed(key, b"tampered", good));
        assert!(!signed(key, data, &good[7..])); // no "sha256=" prefix
        assert!(!signed(key, data, "sha256=zz"));
        assert!(!signed(key, data, ""));
        // what leaves is signed the same way
        assert_eq!(signature(key, data), good);
        let call = store::Call {
            event: "ticket.approved".into(),
            ticket: "SHOP-1".into(),
            project: "SHOP".into(),
            title: "One \"two\"".into(),
            actor: "jane".into(),
            at: 7,
        };
        let sent: Value = serde_json::from_str(&body(&call)).unwrap();
        assert_eq!((sent["event"].as_str(), sent["ticket"].as_str(), sent["by"].as_str(), sent["at"].as_i64()), (Some("ticket.approved"), Some("SHOP-1"), Some("jane"), Some(7)));
        assert!(signed("secret", body(&call).as_bytes(), &signature("secret", body(&call).as_bytes())));
        assert!(send("http://127.0.0.1:9", "", &call).is_err()); // never unsigned
    }

    #[test]
    fn ticket_ids_are_found() {
        assert_eq!(ticket_ids("Fix SHOP-51, see BLOG-7.\nshop-3 no, SHOP-51 again"), ["SHOP-51", "BLOG-7"]);
        assert_eq!(ticket_ids("refs/heads/SHOP-12-webhook"), ["SHOP-12"]);
        assert!(ticket_ids("no id here: SHOP- 51, F-1, -51").is_empty());
    }

    #[test]
    fn pull_requests_and_commits_are_attached() {
        let c = store::open(":memory:");
        let jane = Actor { login: "jane".into(), admin: true };
        store::create_project(&c, &jane, &json!({"key": "SHOP", "name": "Shop"})).unwrap();
        store::create_ticket(&c, &jane, "SHOP", &json!({"title": "One"})).unwrap();
        let links = |c: &Connection| store::ticket_view(c, &jane, "SHOP-1").unwrap()["links"].clone();

        let mut pr = json!({"pull_request": {"html_url": "https://git.example/o/r/pulls/7", "number": 7, "title": "Webhook",
            "body": "For SHOP-1 and SHOP-99.", "state": "open", "merged": false, "head": {"ref": "topic"}}});
        assert_eq!(handle(&c, &pr), ["SHOP-1"]); // SHOP-99 does not exist
        assert_eq!(links(&c)[0]["title"], "PR 7: Webhook");

        // the same pull request, merged: one link still, its label follows
        pr["pull_request"]["merged"] = json!(true);
        handle(&c, &pr);
        assert_eq!((links(&c).as_array().unwrap().len(), links(&c)[0]["title"].as_str()), (1, Some("PR 7: Webhook (merged)")));

        let push = json!({"commits": [
            {"id": "0123456789abcdef", "url": "https://git.example/o/r/commit/0123456789abcdef", "message": "SHOP-1: first line\n\nmore"},
            {"id": "ffff", "url": "javascript:alert(1)", "message": "SHOP-1 bad address"},
            {"id": "eeee", "url": "https://git.example/o/r/commit/eeee", "message": "nothing named"}]});
        assert_eq!(handle(&c, &push), ["SHOP-1"]);
        let all = links(&c);
        assert_eq!(all.as_array().unwrap().len(), 2);
        assert!(all.as_array().unwrap().iter().any(|l| l["title"] == "Commit 0123456: SHOP-1: first line"));
        assert!(handle(&c, &json!({"zen": "ping"})).is_empty());
    }
}

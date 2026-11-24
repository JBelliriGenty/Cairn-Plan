//! Notifications outside the page: e-mail (SMTP) and, for the people listed
//! in the configuration, ntfy. store.rs decides who is told what; a thread
//! sends what is waiting, so that a slow mail server never holds a request.

use crate::store::Notice;
use lettre::message::header::{Header, HeaderName, HeaderValue};
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::collections::HashMap;
use std::time::Duration;

/// Where notifications go besides the page. Both are optional.
pub struct Outside {
    /// Address of the page, for the link in a message; may be empty.
    pub public_url: String,
    pub lang: String,
    pub smtp: Option<Smtp>,
    pub ntfy: Option<Ntfy>,
}

pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub from: String,
}

pub struct Ntfy {
    pub url: String,
    pub token: String,
    /// login -> topic; someone who is not listed gets no push.
    pub topics: HashMap<String, String>,
}

const STATES: [(&str, &str, &str); 6] = [
    ("backlog", "Proposed", "Proposé"),
    ("todo", "To do", "À faire"),
    ("in_progress", "In progress", "En cours"),
    ("waiting", "Waiting", "En attente"),
    ("done", "Done", "Terminé"),
    ("cancelled", "Cancelled", "Annulé"),
];

/// One sentence for one notice, in the language of the instance.
fn sentence(lang: &str, n: &Notice) -> String {
    let fr = lang == "fr";
    match n.kind.as_str() {
        "comment" if fr => format!("{} a commenté :\n{}", n.actor, n.detail),
        "comment" => format!("{} commented:\n{}", n.actor, n.detail),
        "state" => {
            let name = STATES.iter().find(|s| s.0 == n.detail).map(|s| if fr { s.2 } else { s.1 }).unwrap_or(&n.detail);
            if fr {
                format!("{} a passé le ticket à « {name} ».", n.actor)
            } else {
                format!("{} moved the ticket to \"{name}\".", n.actor)
            }
        }
        "wake" if fr => "Ce ticket est de retour : sa date de réveil est arrivée.".to_string(),
        "wake" => "This ticket is back: its wake-up date has come.".to_string(),
        _ if fr => format!("{} vous a assigné ce ticket.", n.actor),
        _ => format!("{} assigned this ticket to you.", n.actor),
    }
}

/// What is waiting, as messages: one per person and ticket, so that a ticket
/// taken, commented and closed in one go is one message, not three.
/// Returns (first notice of the group, subject, text, link).
pub fn messages<'a>(out: &Outside, waiting: &'a [Notice]) -> Vec<(&'a Notice, String, String, String)> {
    let mut groups: Vec<(&Notice, Vec<String>)> = Vec::new();
    for n in waiting {
        let line = sentence(&out.lang, n);
        match groups.iter_mut().find(|(first, _)| first.login == n.login && first.ticket == n.ticket) {
            Some((_, lines)) => lines.push(line),
            None => groups.push((n, vec![line])),
        }
    }
    groups
        .into_iter()
        .map(|(n, lines)| {
            let subject: String = format!("[{}] {}", n.ticket, n.title).chars().filter(|ch| !ch.is_control()).collect();
            let link = if out.public_url.is_empty() { String::new() } else { format!("{}/#/t/{}", out.public_url, n.ticket) };
            (n, subject, lines.join("\n\n"), link)
        })
        .collect()
}

/// "Auto-Submitted: auto-generated" (RFC 3834): this is a notice, not a
/// letter; mail servers and vacation responders treat it as such.
#[derive(Clone)]
struct AutoSubmitted;

impl Header for AutoSubmitted {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("Auto-Submitted")
    }
    fn parse(_: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self)
    }
    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), "auto-generated".into())
    }
}

/// "List-Unsubscribe: <address>": where to turn the notices off.
#[derive(Clone)]
struct ListUnsubscribe(String);

impl Header for ListUnsubscribe {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("List-Unsubscribe")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self(s.into()))
    }
    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), format!("<{}>", self.0))
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The HTML side of a message: the same words, and the link behind a label
/// (mail services rewrite addresses; a bare one becomes a line of noise).
fn html(lang: &str, ticket: &str, text: &str, link: &str, footer: &str) -> String {
    let open = if lang == "fr" { "Ouvrir" } else { "Open" };
    let button = if link.is_empty() { String::new() } else { format!("<p><a href=\"{}\">{open} {}</a></p>", escape(link), escape(ticket)) };
    format!(
        "<div style=\"font-family:system-ui,'Segoe UI',Arial,sans-serif;font-size:14px;color:#131416\"><p>{}</p>{button}\
         <p style=\"color:#5d5f60;font-size:12px\">{}</p></div>",
        escape(text).replace('\n', "<br>"),
        escape(footer)
    )
}

fn mail(s: &Smtp, to: &str, subject: &str, body: String, html: String, settings: &str) -> Result<(), String> {
    let mut builder = Message::builder()
        .from(s.from.parse::<Mailbox>().map_err(|e| format!("sender: {e}"))?)
        .to(to.parse::<Mailbox>().map_err(|e| format!("recipient: {e}"))?)
        .subject(subject)
        .header(AutoSubmitted);
    if !settings.is_empty() {
        builder = builder.header(ListUnsubscribe(settings.to_string()));
    }
    let message = builder.multipart(MultiPart::alternative_plain_html(body, html)).map_err(|e| e.to_string())?;
    // 465: TLS from the first byte; any other port: STARTTLS, refused if the server has none
    let relay = if s.port == 465 { SmtpTransport::relay(&s.host) } else { SmtpTransport::starttls_relay(&s.host) };
    let transport = relay
        .map_err(|e| e.to_string())?
        .port(s.port)
        .credentials(Credentials::new(s.user.clone(), s.password.clone()))
        .timeout(Some(Duration::from_secs(20)))
        .build();
    transport.send(&message).map(|_| ()).map_err(|e| e.to_string())
}

fn push(n: &Ntfy, topic: &str, title: &str, text: &str, link: &str) -> Result<(), String> {
    // JSON publishing: titles and texts are UTF-8, which HTTP headers do not carry well
    let mut body = serde_json::json!({ "topic": topic, "title": title, "message": text });
    if !link.is_empty() {
        body["click"] = link.into();
    }
    let mut req = ureq::post(&n.url).timeout(Duration::from_secs(10)).set("Content-Type", "application/json");
    if !n.token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {}", n.token));
    }
    req.send_string(&body.to_string()).map(|_| ()).map_err(|e| e.to_string())
}

/// An e-mail ready to leave: kept by the caller when it could not be sent,
/// to try it again (`resend`).
pub struct Letter {
    pub login: String,
    pub ticket: String,
    to: String,
    subject: String,
    body: String,
    page: String,
    settings: String,
}

/// Tries a letter again. No SMTP server any more: nothing to do.
pub fn resend(out: &Outside, l: &Letter) -> Result<(), String> {
    match &out.smtp {
        Some(smtp) => mail(smtp, &l.to, &l.subject, l.body.clone(), l.page.clone(), &l.settings),
        None => Ok(()),
    }
}

/// Sends what is waiting. A failure is written to the log; the e-mails that
/// did not leave are returned, to be tried again. A push is not tried again.
pub fn send(out: &Outside, waiting: &[Notice]) -> Vec<Letter> {
    let mut failed = Vec::new();
    for (n, subject, text, link) in messages(out, waiting) {
        if let (Some(smtp), true, false) = (&out.smtp, n.mail, n.email.is_empty()) {
            let footer = if out.lang == "fr" {
                "Vous recevez ce message parce que vous participez à ce ticket. Pour ne plus en recevoir : page Notifications de Cairn Plan."
            } else {
                "You get this message because you take part in this ticket. To stop them: Notifications page of Cairn Plan."
            };
            let body = format!("{text}\n\n{link}\n\n-- \n{footer}\n");
            let page = html(&out.lang, &n.ticket, &text, &link, footer);
            let settings = if out.public_url.is_empty() { String::new() } else { format!("{}/#/n", out.public_url) };
            if let Err(e) = mail(smtp, &n.email, &subject, body.clone(), page.clone(), &settings) {
                eprintln!("notify {}: e-mail about {} not sent: {e}", n.login, n.ticket);
                failed.push(Letter { login: n.login.clone(), ticket: n.ticket.clone(), to: n.email.clone(), subject: subject.clone(), body, page, settings });
            }
        }
        // the topic the person chose, else the one the configuration file gives them
        let topic = if n.topic.is_empty() { out.ntfy.as_ref().and_then(|x| x.topics.get(&n.login)) } else { Some(&n.topic) };
        // the phone rings only for what waits on the person (a question, a ticket that wakes),
        // whichever notice of the message says so; the rest stays in the page and the e-mail
        let asked = waiting.iter().any(|w| w.login == n.login && w.ticket == n.ticket && w.question);
        if let (Some(ntfy), Some(topic), true) = (&out.ntfy, topic, asked) {
            if let Err(e) = push(ntfy, topic, &subject, &text, &link) {
                eprintln!("notify {}: push about {} not sent: {e}", n.login, n.ticket);
            }
        }
    }
    failed
}

/// One message sent right away to the administrator who asks, to check the
/// settings: `kind` is "mail" or "ntfy". The error is the server's own words.
pub fn test(out: &Outside, kind: &str, email: &str, topic: &str) -> Result<(), String> {
    let (subject, text, footer) = if out.lang == "fr" {
        ("Cairn Plan : essai des notifications", "Ce message confirme que les notifications de Cairn Plan partent bien.", "Envoyé depuis la page Réglages.")
    } else {
        ("Cairn Plan: notification test", "This message confirms that the notifications of Cairn Plan are sent.", "Sent from the Settings page.")
    };
    match kind {
        "mail" => {
            let smtp = out.smtp.as_ref().ok_or("no SMTP server is set")?;
            if email.is_empty() {
                return Err("the proxy gave no e-mail address for you".into());
            }
            mail(smtp, email, subject, format!("{text}\n\n-- \n{footer}\n"), html(&out.lang, "", text, "", footer), "")
        }
        "ntfy" => {
            let ntfy = out.ntfy.as_ref().ok_or("no ntfy server is set")?;
            if topic.is_empty() {
                return Err("choose your ntfy topic on the Notifications page first".into());
            }
            push(ntfy, topic, subject, text, &out.public_url)
        }
        _ => Err("invalid request".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(login: &str, ticket: &str, kind: &str, detail: &str) -> Notice {
        Notice {
            question: true,
            login: login.into(),
            email: String::new(),
            mail: true,
            ticket: ticket.into(),
            title: "Le titre\r\nBcc: x".into(),
            actor: "claude".into(),
            kind: kind.into(),
            detail: detail.into(),
            topic: String::new(),
        }
    }

    #[test]
    fn one_message_per_person_and_ticket() {
        let out = Outside { public_url: "https://plan.example".into(), lang: "fr".into(), smtp: None, ntfy: None };
        let waiting = [
            notice("jane", "SHOP-1", "state", "in_progress"),
            notice("jane", "SHOP-1", "comment", "Fait."),
            notice("lea", "SHOP-1", "assigned", ""),
            notice("jane", "SHOP-2", "state", "done"),
        ];
        let m = messages(&out, &waiting);
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].1, "[SHOP-1] Le titreBcc: x"); // no line break in a subject
        assert_eq!(m[0].2, "claude a passé le ticket à « En cours ».\n\nclaude a commenté :\nFait.");
        assert_eq!(m[0].3, "https://plan.example/#/t/SHOP-1");
        assert_eq!(m[1].2, "claude vous a assigné ce ticket.");
        let page = html("fr", "SHOP-1", "claude a commenté :\n<b>Fait</b> & vu", "https://plan.example/#/t/SHOP-1", "Pied");
        assert!(page.contains("claude a commenté :<br>&lt;b&gt;Fait&lt;/b&gt; &amp; vu"));
        assert!(page.contains("<a href=\"https://plan.example/#/t/SHOP-1\">Ouvrir SHOP-1</a>"));
        let en = Outside { public_url: String::new(), lang: String::new(), smtp: None, ntfy: None };
        let m = messages(&en, &waiting[3..]);
        assert_eq!((m[0].2.as_str(), m[0].3.as_str()), ("claude moved the ticket to \"Done\".", ""));
    }
}

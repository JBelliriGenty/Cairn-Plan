# Configuration reference

Everything Cairn Plan reads, key by key. The [README](../README.md) explains
what it does; this page is what you edit.

Cairn Plan reads one JSON file, given as its first argument
(`/etc/cairn-plan/config.json` in the Docker image), and keeps everything
else in one SQLite file.

```json
{
  "listen": "10.0.0.5:9094",
  "trusted_proxies": ["10.0.0.2"],
  "require_groups": [],
  "admin_groups": ["admins"],
  "db": "/var/lib/cairn-plan/plan.db",
  "lang": "fr"
}
```

| Key | Default | |
|---|---|---|
| `listen` | `127.0.0.1:9094` | Address and port, plain HTTP. TLS is the proxy's job. |
| `trusted_proxies` | none, required | IP addresses of the reverse proxies whose identity headers are believed. A request from any other address is refused (403), whatever its headers. |
| `require_groups` | `[]` | If not empty, only people in one of these groups get in. |
| `admin_groups` | `[]` | People in one of these groups are administrators: they create projects and are owners of every project. |
| `db` | `/var/lib/cairn-plan/plan.db` | The SQLite file. Its folder must be writable by the process (user `nobody` in the image). |
| `lang` | browser's | Default language of the page, `en` or `fr`. Each person can switch. |
| `mcp_keys_file` | none | File of the keys accepted on `/mcp`, one `name:key` per line (key of 32 characters or more; lines starting with `#` are ignored). Without it there is no MCP server (`/mcp` answers 404). |
| `mcp_login` | `claude` | The account every MCP call runs as, whichever key is used. Add it to a project like any member. |
| `public_url` | none | Address people open Cairn Plan at (`https://plan.example.org`), for the link in a notification. |
| `smtp_host` | none | SMTP server for notifications by e-mail. Without it no e-mail is sent. |
| `smtp_port` | `587` | `465`: TLS from the start; any other port: STARTTLS (required). |
| `smtp_credentials_file` | none | File holding `user:password` on one line, for the SMTP server. |
| `mail_from` | `Cairn Plan <cairn-plan@localhost>` | Sender of the e-mails; an address your SMTP server accepts. |
| `ntfy_url` | none | Address of an ntfy server for push notifications. Without it no push is sent. |
| `ntfy_token_file` | none | File holding an ntfy access token allowed to publish to the topics below. |
| `ntfy_topics` | `{}` | Who gets a push, and on which topic: `{"jane": "plan"}`. Someone who is not listed gets none. |
| `webhook_secrets_file` | none | File of the secrets that may sign a forge webhook, one `name:secret` per line (same format as `mcp_keys_file`). Without it there is no webhook (`/hooks/git` answers 404). |
| `agent_hook_url` | none | Address called when a ticket is approved (see [Agent webhook](#agent-webhook)). Without it nothing is called. |
| `agent_hook_secret_file` | none | File holding the secret that signs those calls, on one line. |

An unknown key is reported on the first log lines and ignored.

## Settings page

An administrator (someone in one of `admin_groups`) has a "Settings" entry
in the menu. It sets how notifications leave the page: the address of the
page, the SMTP server, the ntfy server; and the webhook of an agent. What
is saved there is kept in the database and wins over the keys `public_url`,
`smtp_*`, `mail_from`, `ntfy_*` and `agent_hook_*` of the file, which stay
useful to prepare an installation. The password, the token and the secret
are never shown again; leaving their field empty keeps what is saved.
"Send a test" sends one message to the administrator (for the agent, one
"ping" call) with the settings that are saved, and shows the answer of the
server if it fails.

The file keeps what is needed before anyone can sign in: `listen`,
`trusted_proxies`, the groups, `db`, and the keys of the MCP server and of
the webhooks.

## Sign-in

Cairn Plan has no password of its own. The reverse proxy signs people in
and passes who they are:

| Header | Use |
|---|---|
| `Remote-User` | the login (lower case letters, digits, `.`, `_`, `-`); it is what you type to add a member to a project |
| `Remote-Groups` | comma-separated groups, matched against `require_groups` and `admin_groups` |
| `Remote-Name`, `Remote-Email` | shown next to the login |

These are the headers Authelia sends through Traefik's `forwardAuth`. Two
things keep them honest: the proxy must overwrite any such header sent by a
browser (Traefik does, with `authResponseHeaders`), and nothing but the
proxy must reach the port (`listen` on a private address, plus a firewall
rule). `trusted_proxies` is the last line: a direct request is refused.

`/healthz` answers `ok` to anyone, for probes.

## MCP server

`/mcp` speaks the Model Context Protocol over HTTP (JSON answers, no
stream) and is the one path that does not go through the proxy's sign-in:
the caller sends `Authorization: Bearer <key>`, a key of `mcp_keys_file`.
Route it at the proxy without `forwardAuth`, with a rate limit.

The key only says which client calls (its name is written to the log with
each tool call). The account is always `mcp_login`, an ordinary member:
it sees the projects it was added to and, as a contributor, can neither
approve a ticket nor change the text of an approved one. Do not make it
an owner of a project, nor a member of `admin_groups`: it could then
approve its own work.

Tools: `list_projects`, `list_tickets` (approved tickets by default),
`get_ticket` (with who approved it, when, and the edits of its text
since), `take_ticket`, `wait_ticket` (a question: the ticket waits),
`comment_ticket`, `add_link`, `finish_ticket` (with the minutes worked),
`propose_ticket`, `ask_owner` (a question to a project owner: a ticket
created "waiting" and assigned to them; it shows first in their « For me »
page, where they answer and move it to "done" or "todo").

## Notifications

The people who take part in a ticket (its author, its assignee, those who
commented) are told when someone else comments it, changes its state or
assigns it to them. Someone removed from the project is not told any
more. The notices are in the page ("Notifications", with the number of
unread ones); opening a ticket marks its notices as read.

With `smtp_host`, each notice is also sent by e-mail to the address the
proxy gives (`Remote-Email`), unless the person turned it off on the
Notifications page. With `ntfy_url`, the people listed in `ntfy_topics`
get a push as well, and so does each member who chose a topic on the
Notifications page (it must start with the prefix set on the settings
page, if any: give the ntfy token write access to those topics only).
Changes made within a few seconds to the same ticket
make one message. A message that cannot be sent is written to the log and
not sent again; the notice stays in the page.

## Forge webhooks

`/hooks/git` receives the webhooks of a Git forge (Forgejo, Gitea, GitHub)
and, like `/mcp`, does not go through the proxy's sign-in. A pull request
or a commit that names a ticket (`SHOP-51` in the title, the text, the
branch or the commit message) is attached to that ticket as a link; when
the pull request is merged or closed, the label of the link says so.

In the forge, on a repository or on a whole organisation: target
`https://<your name>/hooks/git`, content type `application/json`, events
"push" and "pull request", and as secret one of the secrets of
`webhook_secrets_file`. The forge signs each body with it (header
`X-Hub-Signature-256`); a request with a wrong signature is refused (401).
The name in front of the secret is written to the log with each call.

The forge is not a member: it can only add links, to tickets that exist,
in the name `forge` in the history of the ticket.

## Agent webhook

The other way round: Cairn Plan calls an address of yours, so that an
agent takes a ticket as soon as it may, instead of looking for work at
fixed hours. Set the address and a secret (16 characters or more) on the
settings page, section "Agent", or with `agent_hook_url` and
`agent_hook_secret_file`. The [agent](../agent/README.md) shipped with
Cairn Plan listens to it.

Two events are sent, as a `POST` with a JSON body:

| `event` | When |
|---|---|
| `ticket.approved` | a project owner moved a ticket to "To do" (or created it there) |
| `ticket.answered` | whoever approved a ticket that is "Waiting" commented it |

```json
{"event": "ticket.approved", "ticket": "SHOP-52", "project": "SHOP",
 "title": "Fix the header", "by": "jane", "at": 1795000000}
```

The body is signed like a forge signs its own: header
`X-Hub-Signature-256: sha256=<hex>`, the HMAC-SHA256 of the raw body under
the secret; the event is also in `X-Cairn-Event`. The receiver must check
the signature and then read the ticket itself (MCP `get_ticket`,
`take_ticket`): the call carries no text to act on, only which ticket to
look at.

Calls leave within ten seconds, outside any request. One that fails is
written to the log (`hook out: ticket.approved SHOP-52 not sent: ...`) and
not sent again: the ticket stays "To do" for whoever takes it next.

## Attached files

A contributor attaches files to a ticket, any kind, up to 25 MB each;
whoever may read the ticket may read them. They are kept in a folder
`files` next to the database, one file per attachment, named by a number
(the name people see is in the database). Images (PNG, JPEG, GIF, WebP)
and PDF open in the browser; anything else is downloaded, so that an
attached HTML or SVG file never runs as a page of your site. A proxy in
front must let a request body of 25 MB through.

## Backup

Copy the SQLite file while the service runs with
`sqlite3 plan.db ".backup plan-backup.db"`, or stop the container and copy
the file. Copy the `files` folder next to it as well: it holds the
attached files.

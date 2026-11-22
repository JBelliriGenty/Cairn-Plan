# Changelog

## 0.16.4

- A sub-ticket closed ("Done" or "Cancelled"), by an agent or a person,
  sends `ticket.closed`. The agent then starts the part that waited for it
  (the writer once the code is done) and closes the parent once every part
  is, instead of waiting for its hourly catch-up.

## 0.16.3

- Agent: "out of usage credits" (a model the subscription does not cover)
  pauses the role until the next day, like a usage limit, instead of
  failing every ticket of that role. The example team leads with Opus.

## 0.16.2

- Authelia's userinfo answers with no audience at all: the audience and
  client id of an OAuth access token are now read in the token itself (a
  JWT), once the provider has accepted it. claude.ai was still refused
  after sign-in with 0.16.1.

## 0.16.1

- An OAuth access token whose audience is the address of `/mcp` (the
  resource the MCP client asked for, as Authelia answers on userinfo) is
  accepted; only the client id was before, so claude.ai was refused after
  sign-in. A refusal now logs the audience it saw.

## 0.16.0

A team of agents per project, each role with its own account, model,
tools, paths and budget. The approval of a project owner on a parent
ticket covers the sub-tickets the team lead cuts from it, never more.

- Sub-tickets: a ticket may have a parent of the same project (one level,
  8 at most). Created "To do" by whoever took the parent, a sub-ticket is
  approved with it, as long as the parent was approved and its text not
  changed since by someone else. Its text and its agent role are frozen
  like any approved ticket. Cancelling the parent cancels the sub-tickets
  still open. A progress bar (sub-tickets done out of those not cancelled)
  on the ticket page, the board card and the list.
- Field "Agent" on a ticket: the role of the agent that builds it (`dev`,
  `doc`); empty: the team lead.
- MCP: `create_subticket`; `get_ticket` of a sub-ticket gives the approval
  of its parent. `mcp_accounts` maps a key to its own account, one per
  role. `cairn-plan member <config> <KEY> <login>` adds an account to a
  project as a contributor.
- Agent: `AGENT_TEAMS` and [agent/team.json](agent/team.json). The lead
  (Claude Code without Bash nor writing) splits the ticket and builds
  nothing; each sub-ticket goes to the agent of its role, under its
  account and model, with the parent as the frame it may not go beyond;
  the files it changed are checked against the paths of its role before
  any push; a role that "waits" (the writer) starts once the other parts
  are closed and reads their branches; the parent is closed once every
  part is. One daily budget per role, the role in front of each line of
  the log, its model and cost at the end of each report.

## 0.15.0

Sign-in with OAuth on the MCP server, for remote clients such as
claude.ai custom connectors and Claude Cowork.

- New keys `oidc_issuer` and `oidc_client_id`: `/mcp` then also accepts
  an access token from that OpenID Connect provider, and the call runs as
  the person signed in (their projects, their role in each, administrator
  if in `admin_groups`) instead of the fixed `mcp_login` account. Keys of
  `mcp_keys_file` work as before.
- `/.well-known/oauth-protected-resource` (RFC 9728) names the provider,
  and the 401 of `/mcp` points to it (`resource_metadata`).
- A token is checked with the provider's userinfo endpoint and must have
  been issued to `oidc_client_id`; a refusal is logged with its reason.

## 0.14.0

First version published on GitHub, with images for the server and the
agent (`ghcr.io/jbellirigenty/cairn-plan`, `cairn-plan-agent`).

The agent does the work an approved ticket asks for, and lets whoever
merges decide what needs a person.

- `agent/rules.json` has two levels. `blocking` (CI workflows, `.env`,
  secrets): no pull request, the ticket waits with the reason, as before.
  Everything else in the rules (Dockerfiles, compose and deployment files,
  migrations, destructive schema changes, more than 400 lines or 20 files):
  the pull request is opened all the same, its text says it is for a
  person to merge, and a merging bot that reads the rules
  leaves it open.
- Additive schema changes (`CREATE TABLE`, `ALTER TABLE … ADD COLUMN`) and
  dependency manifests are no longer in the rules; `DROP`, `RENAME`,
  `ALTER COLUMN` and `TRUNCATE` are.
- A ticket where Claude asked a question during the run is left waiting,
  even if its state was changed again before the run ended (it used to be
  closed as done with no change).

## 0.13.0

Questions to a project owner. A contributor (an AI agent through the MCP
tool `ask_owner`, or a person) may create a ticket in state "waiting"
assigned to a project owner, never to anyone else: something only the
owner can do or decide. The owner is notified as for any assignment.

- « For me » page (`#/m`, first entry of the menu, `GET /api/mine`): the
  open tickets assigned to you in every space, the questions waiting for
  your answer first, their number on the menu entry.
- A waiting ticket assigned to you opens on the question, with an answer
  box and three buttons: done (close it), answered: carry on (an owner
  approves what the ticket says comes next: "todo"), only send the answer.
- Descriptions and comments show fenced code blocks (```) as they are,
  each with a copy button; where the clipboard is refused, the block is
  selected for Ctrl+C.
- `cairn-plan import`: a draft may set the project description
  (`description`) and add members (`membres`: `{"login": "role"}`).
- Agent: a question to the owner is never taken, even once approved; what
  follows it is left to whoever asked (a session, the night routine).

## 0.12.2

A ticket or a project in front follows the changes others make (state,
comment, link) without reloading the page: checked every 30 s and as soon as
the tab comes back in front, drawn again only when something changed and
nobody is typing in the page.

## 0.12.1

Comments: web addresses (http, https) are links, opened in a new tab. The
rest of a comment stays plain text.

## 0.12.0

Agent: rules for what its pull requests may contain (`agent/rules.json`,
`AGENT_RULES_FILE`): no CI workflow, Dockerfile, compose or deployment
file, secret, dependency manifest or lock, database migration or schema
change, binary file, at most 400 changed lines and 20 files. A change that
breaks them is pushed to the branch of the fork but gets no pull request;
the ticket waits with the reason. The same file is meant for whatever
merges the pull requests.

## 0.11.2

Agent: it catches up with what a lost call left behind, when it starts
and after an hour without a call (`AGENT_SWEEP_MIN`): approved tickets
nobody is assigned to, and waiting tickets answered by whoever approved
them. A ticket assigned to someone is left to them.

## 0.11.1

Agent: the prompt tells Claude that the ticket is already taken and will
be closed for it. On the first real ticket it tried to take it, was
refused as intended, and reported the ticket as not taken.

## 0.11.0

A ticket is taken as soon as it is approved. Cairn Plan calls a webhook
(address and secret on the Settings page, body signed with HMAC-SHA256
like a forge's) when a project owner approves a ticket, and when whoever
approved a waiting ticket answers it. New folder `agent`: a container
that listens to it, runs Claude Code on the ticket in a clone of the
project's repository, opens a pull request that names the ticket and
closes the ticket with the report. One ticket at a time, a daily cap and a
daily budget; it merges and deploys nothing.

## 0.10.0

Settings page, for administrators: the address of the page, the SMTP
server and the ntfy server are set from the screen, with a button that
sends a test and shows what the server answered. What is saved there wins
over the configuration file, which keeps what is needed before anyone can
sign in. Each member chooses the ntfy topic of their own notifications on
the Notifications page; the administrator may impose a prefix.

## 0.9.1

Menu: the projects of a space are shown one step in, along a rule, under
"Projects"; an arrow folds them away.

## 0.9.0

Time. Each member logs the time worked on a ticket (a duration, a day, a
note); the ticket shows the total. Next to it, the time the ticket spent
in each state, read from its history. A new state, "Waiting", for a
ticket that waits for an answer: its time is counted apart from the time
in progress. MCP: `wait_ticket` asks a question and puts the ticket on
hold, `take_ticket` resumes it, `finish_ticket` takes the minutes worked.

## 0.8.0

Files: a contributor attaches files to a ticket (any kind, up to 25 MB
each); every member of the project reads them. Images and PDF open in the
browser, anything else is downloaded, never shown as a page. The files
are kept in a `files` folder next to the database: back it up with it.

## 0.7.2

Notification e-mails have an HTML side, with the link behind a label, and
say that they are automatic (`Auto-Submitted`, `List-Unsubscribe` to the
Notifications page): the first ones, in plain text, were filed as junk.

## 0.7.1

A request without a name or an e-mail address no longer erases the ones
the proxy gave before: the address is what notifications are sent to.

## 0.7.0

Notifications: the people who take part in a ticket (its author, its
assignee, those who commented) are told when someone else comments it,
changes its state or assigns it to them. A "Notifications" entry in the
menu counts what is unread, kept up to date while the page stays open;
opening a ticket marks its notices as read.
Optionally the same notices leave by e-mail (SMTP, each person can turn
it off) and by ntfy for the people listed in the configuration. One
message per person and ticket, not one per change.

## 0.6.0

Forge webhooks at `/hooks/git`: a pull request or a commit that names a
ticket (`SHOP-51` in its title, text, branch or message) is attached to
it, and the label of the link follows the pull request (merged, closed).
Works with Forgejo, Gitea and GitHub: the body is signed with a shared
secret (`webhook_secrets_file`).

## 0.5.1

MCP: every tool argument is described, and the server says what a client
had to guess (a proposal is the state "backlog", what "open" counts, times
are Unix seconds). No change of behaviour.

## 0.5.0

MCP server at `/mcp`: an AI agent lists the tickets approved by a project
owner, takes one, comments, links, reports and closes it, or proposes a
new one. Every call runs as one ordinary account (`mcp_login`), which
cannot approve a ticket nor change the text of an approved one.

## 0.4.1

A lost sign-in no longer leaves the page stuck on "Failed to fetch": the
page reloads so that the browser goes through the sign-in again. The menu
always names the space, even when there is only one.

## 0.4.0

Milestones: a ticket can be filed under a milestone (a phase, a version),
with a filter on the project page. Import of a history draft with
`cairn-plan import`: one ticket per past lot, dated by its start, with its
links; a replay creates nothing twice.

## 0.3.0

Board: the tickets of a project in one column per state; drag a card to
change its state (the approval rule still applies). Search and filters by
assignee and priority, in the list and on the board. One space at a time:
a switch in the menu, and the menu and home page only show its projects.

## 0.2.0

Spaces: a project can be filed under a space (a company, an area). The home
page and the menu group projects by space. Administrators file projects;
rights stay per project. Project owners can rename a project.

## 0.1.0

First version: private projects with three roles (reader, contributor,
owner), tickets with state, priority, assignee, start and end dates, links,
comments and history, approval reserved to project owners, sign-in by the
reverse proxy's identity headers, page in English and French.

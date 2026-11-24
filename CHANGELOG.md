# Changelog

## 0.31.1

- A run reported through an OAuth token (the Claude of a member) is now stored
  under `<login>·claude`, like what that Claude writes elsewhere: the "Agents"
  page and the team page show an agent, not the person. A member and their
  Claude see the runs of their projects as before; a run id of the person and
  one of their Claude are two runs. Runs already stored keep their login (no
  schema change); static keys do not change.

## 0.31.0

- A person's Claude (an OAuth access token on `/mcp`) no longer acts with the
  rights of the person. It is never an administrator, and its role in a
  project is capped at contributor: it cannot approve a ticket, change the
  text of an approved one or manage members, even if the person is an owner
  or in `admin_groups`. Keys in `mcp_keys_file` do not change.
- What it writes carries the name `<login>·claude` (events, comments, files,
  notices, calls to the agent webhook), so a reader tells the person from
  their Claude. Assignees, members, runs and the author of a ticket keep the
  plain login. The page draws such a name as a square disc, like an agent.
- A person gets no notice for what their own Claude did, as for what they
  did themselves: the notice is written under `login·claude`, but the person
  is left out by login.
- Time entered through `finish_ticket` (minutes) stays counted on the
  person's login, at their hourly rate.
- The role `list_projects` and the project list return, and the money of the
  time report, follow the same cap.

## 0.30.0

- Team page: two new drawings, `shield` (a shield with a padlock) and
  `scales`. The screen test checks that a run shows on its member's sheet
  only.

## 0.29.0

- Drafts: what someone types in a ticket (comment, answer to a question,
  description) and in the description of a new ticket is kept in the browser
  (`localStorage`, one key per ticket and field) while they type. It comes
  back when the page is drawn again, with the mark "Draft restored", and is
  removed once it is sent.
- A lost sign-in at the proxy no longer reloads the page (that wiped the text
  being typed). A banner says the session has ended and opens the sign-in in a
  new tab; back in the tab, the person sends again. The background requests
  (notifications, the ticket in front) show the banner as soon as the session
  ends, before anyone presses "Save".

## 0.28.0

- Team page: a line under the title gives the runs of the members since
  midnight and what they cost ("≈" when part of it is an estimate).
- A member's `agents` may end with `*`: a prefix, for agents started on
  demand under many names.
- Two new drawings: `toolbox` and `binoculars`.

## 0.27.2

- A team's entry in `AGENT_TEAMS` may give a role a `note`: text that only that
  project's team needs. The agent adds it to the role's text from `team.json`.
- The texts read by the team agents (`team.json`, MCP tool examples) name no
  project: examples use `APP`, and the developer role no longer lists parts of
  one product.

## 0.27.1

- A run belongs to the account that reports it: its key is the pair
  (account, `run_id`). Another account that sends the same `run_id` writes a
  run of its own and can no longer take an id in advance or touch the run of
  someone else. The table of runs is rebuilt once at start; the old one is kept
  as `runs_before_0_27_1`, and nothing is lost.
- Time page: a total in the currency of the page shows, under the pointer, what
  it is made of by currency ("800 CHF + 300 € + 9.04 $"), when the amounts come
  from more than one currency.

## 0.27.0

- Team page `/team`: the agents drawn as characters, their state at a glance
  (working, waiting, resting), what each one did (runs, results, costs) and
  what is in its queue (open tickets assigned to it). It reads again every
  20 seconds; "Replay the last 24 hours" plays the day in one minute. The
  members come from the new configuration key `team` (docs/configuration.md,
  "Team page"). New route `GET /api/team`: the members and their queues,
  limited to what the viewer may see.

## 0.26.1

- A run that reports no cost takes an estimate from its tokens and a
  table of prices by model (dollars per million tokens: input, output,
  cache read, cache write). The page shows it with "≈" and "(estimate)". The
  starting prices were read on the official pricing page on 2026-11-24;
  an administrator adjusts them in the "Currencies" block of the Settings
  page, which now has a "Save" button. The table of prices and the table of
  currencies show the date of their last change.
- A run that reports its cost for everything it started (a session) loses the
  estimate of the runs it started, which are counted on their own: a sum over
  the runs counts nothing twice. Runs have `cost_counted` and `cost_estimated`.
- MCP `report_run` takes `tokens_cache_read` and `tokens_cache_write` (part of
  `tokens_in`). Two columns are added to `runs`.
- The hourly rate of an agent is computed from counted costs, estimates
  included. A card without a rate says "not measured yet".

## 0.26.0

- Currencies. Amounts are shown in one ISO currency (EUR, USD, CHF, GBP...),
  chosen by an administrator in the new "Currencies" block of the Settings
  page, with what one dollar is worth in each currency (a table to adjust
  when the rates move). The dollar is the pivot: agents cost dollars. A
  setting that held a sign ("€") reads as its code.
- Hourly rate of each member. "My profile" has a "My hourly rate" block:
  the amount and its currency. Only the member, the administrators and the
  owners of their projects see it. An administrator can still set the rate
  of someone else from the Time page. The Time page converts every rate to
  the currency of the page and adds people and agents in one amount.
- Hourly rate of each agent, computed: the dollars its runs cost over the
  last 30 days, divided by the time of those runs, in the currency of the
  page. It shows on the cards of the "Agents" page. The cost of the page
  is shown in the same currency.
- API: `GET /api/agent-rates`; `POST /api/rates` takes `rate` and
  `rate_currency` (the caller, or anyone for an administrator), `currency`
  and `fx` (administrators); `GET /api/me` has a `money` block. The Time
  report gives `rates` converted, `rates_own`, `per_usd` and `currencies`.
- New column `users.rate_currency`, added at start. Nothing is removed.

## 0.25.0

- Wake-up date: new field "Wake up" of a ticket. Until that day the ticket
  is out of "For me" and no agent takes it (`take_ticket` refuses). On the
  day, the date is cleared, the people of the ticket are told, and an
  approved ticket is announced to the agent again.
- New page "Agent" for administrators (button "What the agent is doing" of
  the Settings page): the ticket in hand, the tickets and dollars of the
  day against the caps of each role, the roles stopped until tomorrow, the
  last lines of the agent's log.
- Agent: new `GET /status` next to `/hook`, answered only with the secret
  of the webhook (`Authorization: Bearer`). The agent leaves a ticket that
  sleeps when it looks for missed calls.
- Phone (ntfy): a push is sent only for what waits on the person: a
  question asked to them (`ask_owner`, `wait_ticket`) and a ticket that
  wakes. Everything else stays in the page and the e-mail.
- API: tickets carry `wake_date`; `GET /api/agent` (administrators).
- Narrow screens: the links of the menu keep their width ("For me" was
  squeezed); the row scrolls.

## 0.24.0

- New page "Agents" (menu, under the projects): what the agents do, as
  they report it. A sentence (how many are at work), what runs now, one
  line per piece of work with its verdict (a click opens it: who did
  what, with which model, how long, how many tokens, what it cost), one
  card per agent. Period: 24 hours, 7 days or 30 days. It follows the
  runs live, like the other pages.
- MCP: new tool `report_run`. An agent reports a run when it starts and
  again when it ends, with the same `run_id`; what the second report
  leaves out is kept. Only a summary (agent, task, state, model, tokens,
  cost in dollars, times, ticket, project, parent, verdict): never the
  instruction nor the output of a command. An account writes its own runs
  only: another account's `run_id` is refused.
- API: `GET /api/runs?since=<date or seconds>&agent=<name>&project=<KEY>`.
  A run of a project is seen by the members of that project, like its
  tickets; a run without a project is seen by administrators only.
- New table `runs`, created at start. No migration of the tables that exist.
- The agent (`agent/agent.py`) reports one run per turn of work on a ticket:
  "running" when it takes the ticket, then "ok", "failed" or "waiting" with
  the cost Claude Code counted (`total_cost_usd`).

## 0.23.0

- MCP: new tool `close_question` (`id`, `report`). An agent closes a
  question it asked itself with `ask_owner`: the report is written as a
  comment and the ticket becomes "done". Refused when the ticket was not
  opened as a question by this same account, or when it is no longer
  "waiting". The same rule holds in the store.

## 0.22.0

- New page "Search" (menu, under "For me"): the tickets of every project
  you see whose id, title, description or comments hold a text, newest
  first, with the text around the match.
- MCP: new tool `search_tickets` (`query`, optional `project`), so that an
  agent finds what was decided or done before.
- Ticket page: the description reads like a comment (blocks with "Copy",
  diagrams drawn). "Edit the description" opens the field.
- Forge webhook: a commit pushed to the main branch that names a ticket and
  adds or changes a Markdown file attaches that document to the ticket
  (block "Documentation" of the ticket page). `CHANGELOG.md` is left out.
- API: `GET /api/search?q=<text>&project=<KEY>`.

## 0.21.0

- New command `cairn-plan export <config.json> <folder>`: a consistent copy
  of the database, safe while the server runs. The 14 newest copies are
  kept. Run it every night before your backup.
- A project in full as JSON: button "Export" of the Members page, or
  `GET /api/projects/<KEY>/export` (tickets with comments, links, history
  and time), for any member of the project.
- An e-mail or a call to the agent that fails is tried again one minute
  later, then five minutes later, then given up. The log says each try.
  The tries are kept in memory: a restart forgets them.
- The image is built only if the page passes a test in a headless browser
  (`tests/screen.py`: projects, a ticket with a diagram, "For me",
  notifications, time). On x86-64 only.

## 0.20.0

- "For me" also lists the questions asked on a ticket you approved (the
  ticket stays assigned to whoever asked), until you answer in a comment.
  Each line says how long the ticket has been in its state and, for a
  question, shows the last comment: it may change what is asked.
- Ticket page: a question on a ticket you approved is shown at the top,
  with a field to answer. A question assigned to you shows the update its
  author wrote after it.
- Notifications: a ticket that starts ("In progress") no longer notifies
  anyone. The page shows one line per ticket: its newest notice, and how
  many came before.
- New ticket: the form takes the description too. A project owner can
  "Create and approve" in one gesture.
- List of tickets: a column that no ticket shown fills (priority, assignee,
  start, end) is left out. The priority filter shows only when a ticket of
  the project has a priority.
- API: `GET /api/mine` adds `assignee`, `since` (when the ticket entered
  its state) and `last` (author and first 300 characters of the last
  comment).

## 0.19.0

- Profile photo: new page "My profile" (bottom of the menu). Everyone
  chooses a photo (PNG, JPEG or WebP) or removes it.
- Framing: once the file is chosen, a dialog shows the photo under the disc
  it will fill. Drag the photo (or use the arrow keys) to place it, move
  the slider to zoom, then save. The page sends that square, 256 pixels
  wide.
- The photo replaces the initials wherever a person is shown: tickets,
  board, comments, history, members, Time page. No photo: the initials, as
  before. The shape stays: a disc is a person, a square is an agent.
- An administrator sets the photo of the agents (MCP accounts) on the same
  page: an agent never signs in to the page.
- An administrator can remove the photo of anyone (block "People" of the
  same page), never choose it. The log names who removed whose photo.
- Photos are kept in a folder `avatars` next to the database, one file per
  account, 1 MB at most. The server reads the first bytes: anything that is
  not a PNG, a JPEG or a WebP is refused. Add the folder to your backup.
- API: `GET /api/me` lists who has a photo (`photos`: login → version), so
  the page never asks for a photo that does not exist;
  `GET /api/avatars/<login>`, `POST /api/avatars/<login>` (the bytes of the
  image, or `{"delete": true}`).

## 0.18.0

- New page "Time" for each project (button "Time" of the project) and for
  each space (entry "Time" of the menu). It says the hours logged over the
  period (7, 30, 90 days, or all), split between people and agents, and the
  tickets done with their average lead time. Three views: Summary (bars per
  day, week or month, then who and which project or ticket), Timesheet
  (people in rows, days, weeks or months in columns, totals) and Journal
  (the entries day by day).
- Hourly rates: an administrator sets what an hour of each person costs
  and the currency, at the bottom of the page. The page multiplies the rate
  by the time logged. Only administrators and project owners see amounts
  (for a space: owners of every project shown).
- Agent cost: the page adds up the cost agents write at the end of their
  report, dated by the comment. The agent now writes it for every ticket,
  not only in a team.
- People are shown with their initials on a disc, one colour per person; a
  square is an agent (an MCP account).
- API: `GET /api/projects/<KEY>/time`, `GET /api/spaces/<space>/time`,
  `POST /api/rates`.

## 0.17.2

- The agent log names the closing that woke a part:
  `[doc] SHOP-73: taken (ticket.closed SHOP-72)`, no longer
  `taken (ticket.approved)`. A part is still queued only once, whatever
  woke it.

## 0.17.1

- A diagram that fails to draw no longer makes the other diagrams of the
  same page fail: it removes only its own temporary element, not those of
  the diagrams drawing at the same moment.

## 0.17.0

- A ```mermaid block in a description or a comment is drawn as a diagram
  (SVG), light or dark with the system. The "Copy" button copies the
  source. Any other block is unchanged.
- Mermaid 11.12.0 is part of the binary (`www/mermaid-11.12.0.min.js`, MIT),
  with no CDN. The browser fetches it only when a page shows a diagram.
- Mermaid runs with `securityLevel: "strict"`: no HTML and no click in a
  diagram.
- Content-Security-Policy: the page (and only the page) adds
  `style-src 'self' 'unsafe-inline'`, because Mermaid writes a `<style>`
  element in each SVG. `script-src` stays closed.
- A syntax error shows the source as text with a short message.

## 0.16.5

- Agent: the lead no longer gives a sub-ticket a part outside the
  project's repository (another repository, a secret, an account, a
  gesture of the owner): the agents only clone that repository, so such a
  part could only wait. The lead writes it in its report or proposes it.

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

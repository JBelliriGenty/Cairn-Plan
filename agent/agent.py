#!/usr/bin/env python3
"""Cairn Plan agent: takes a ticket as soon as a project owner approves it.

Cairn Plan calls /hook (signed webhook, see docs/configuration.md of Cairn
Plan). For each call, one at a time: take the ticket (MCP), clone the
repository of its project, let Claude Code work in the clone, push a branch,
open a pull request that names the ticket, close the ticket with the report.
If Claude asks a question (wait_ticket) the ticket waits, and the answer of
whoever approved it starts the work again on the same branch.

Two users. This program (root) holds the secrets of the forge and of the
webhook. Claude Code runs as "worker", which sees neither: what it commits
comes back as a git bundle, a plain file, and never as a repository whose
configuration or hooks root would run.

A project may have a team instead (AGENT_TEAMS, roles in team.json): each
role has its own Cairn Plan account and key, model, tools, paths it may
change and daily budget. The lead splits an approved ticket into
sub-tickets, one per role, which its approval covers; each is built by the
agent of its role, and the parent is closed once they all are.
"""
import base64
import collections
import datetime
import fnmatch
import hashlib
import hmac
import http.server
import json
import os
import pwd
import queue
import re
import shutil
import socket
import stat
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import zoneinfo

DIR = os.path.dirname(os.path.abspath(__file__))
EVENTS = ("ticket.approved", "ticket.answered", "ticket.closed")
# Under this free space in the work volume, the tools Claude installed in its home go (it installs them again).
KEEP_FREE = 10 * 2**30
# The agent role of a ticket this program never takes: work it cannot do (a host, a CI workflow, a deployment).
ROUTINE = "routine"
# What Claude may use. Taking and closing the ticket stay with this program.
TOOLS = ["Read", "Edit", "Write", "Glob", "Grep", "Bash",
         "mcp__plan__get_ticket", "mcp__plan__comment_ticket", "mcp__plan__wait_ticket", "mcp__plan__propose_ticket"]
# The first words of the comment left when Claude's usage limit stopped a run: the sweep looks for them.
LIMITED = {"en": "The agent reached Claude's usage limit", "fr": "L'agent a atteint la limite d'usage de Claude"}
# A technical failure (FAILED) or a change the rules refuse (REFUSED): the sweep tries the ticket again, twice at most,
# instead of asking the owner (rule of 2026-11-23: agents fix, a machine checks; only the closed list is the owner's).
FAILED = {"en": "The agent stopped before the end", "fr": "L'agent s'est arrêté avant la fin"}
REFUSED = {"en": "The work is done, but the rules of the agent forbid opening this pull request",
           "fr": "Le travail est fait, mais les règles de l'agent interdisent d'ouvrir cette pull request"}
RETRIES = 2
# the merge bot's comment that sends a closed ticket back to its agent: the second agent found « écarts » in its pull
# request, or the pull request is in conflict with main (its rework). Its first words, from its account
# (AGENT_REWORK_FROM): the sweep takes the ticket again on the same branch, the ticket stays closed.
REWORK = "Reprise demandée à l'agent"
REWORK_DAYS = 3
# The last line of a lead's report that needs no sub-ticket (prompt-chef.md): the ticket is closed with the report.
NOTHING = "NOTHING TO BUILD"
REWORK_PROMPT = """

## Taken again: the merge bot did not merge the pull request

This ticket was closed and its pull request opened, but the merge bot (the machine that checks and merges the pull requests
of the agents) did not merge it. Its comment below is part of the request: do what it asks, on this branch, and nothing
else. The ticket stays closed: do not call `wait_ticket`, it would be refused; decide, and say in your report what is left.
{merge}
<merge-bot>
{ask}
</merge-bot>
"""
MERGE_CONFLICT = ("A merge of origin/{base} into this branch is in progress and stopped on conflicts in: {files}. "
                  "Resolve each file (keep the work of the ticket and that of {base}), remove every conflict marker, "
                  "then run the checks of the repository. The merge is committed for you if you do not commit it.\n")
MERGE_DONE = "origin/{base} is already merged into this branch, without conflict.\n"
# A question of a building role sent to a review role (forward): the part goes again once no review of its parent is open.
RETRY = "retry after a failure"
FORWARDED = {"en": "Question sent to", "fr": "Question envoyée au rôle"}
LIMIT = re.compile(r"(?i)usage limit|session limit|weekly limit|hit your .{0,20}limit|rate limit|quota|credit balance|usage credits")
TEXT = {
    "en": {"failed": FAILED["en"] + ": {why}\n\nIt tries this ticket again by itself at the next sweep (twice at most; then it waits "
                     "for a session, see the agent's log); nothing to answer.",
           "limited": LIMITED["en"] + ": {why}\n\nIt takes this ticket again by itself once the limit is reset{when}; nothing to answer.",
           "lost": "The agent was restarted while it worked on this ticket: its run is lost. It takes the ticket "
                   "again by itself, from its branch if it pushed one.",
           "forwarded": FORWARDED["en"] + " the {role} role: {id}. This ticket goes on by itself once {id} is closed; nothing to answer.",
           "review_ask": "Answer in your opinion (section « Règles pour le code »): {id} reads it and goes on by itself.",
           "pr": "Pull request: {url}", "ticket": "Ticket {id}.",
           "reworked": "Taken again at the request of the merge bot: {n} commit(s) pushed on {branch}, the pull request is up to date.",
           "reworked_none": "Taken again at the request of the merge bot: nothing new pushed on {branch}.",
           "rework_failed": "Taken again at the request of the merge bot, without success: {why}\n\nThe pull request stays open for a person.",
           "refused": REFUSED["en"] + ": {why}.\n\n"
                      "The commits are on branch {branch} of the fork. The agent tries again by itself at the next sweep, "
                      "another way, without these files (twice at most); nothing to answer.",
           "manual": "To be merged by a person, not automatically: {why}.",
           "breach": {"files": "files it may not change: {v}", "binary": "binary files: {v}",
                      "lines": "{v} lines changed, at most {max}", "count": "{v} files changed, at most {max}",
                      "sql": "a destructive database schema change: {v}", "outside": "files role {role} may not change: {v}"},
           "nothing": "the project lead created no sub-ticket. Its report:\n\n{report}",
           "closed": "Every sub-ticket is closed:\n\n{list}", "states": {"done": "done", "cancelled": "cancelled"}},
    "fr": {"failed": FAILED["fr"] + " : {why}\n\nIl reprend seul ce ticket au prochain passage (deux essais au plus ; ensuite il "
                     "attend une session, voir le journal de l'agent) : rien à répondre.",
           "limited": LIMITED["fr"] + " : {why}\n\nIl reprend seul ce ticket après la remise à zéro{when} : rien à répondre.",
           "lost": "L'agent a redémarré pendant qu'il travaillait sur ce ticket : son travail en cours est perdu. "
                   "Il reprend le ticket seul, depuis sa branche s'il en a poussé une.",
           "forwarded": FORWARDED["fr"] + " {role} : {id}. Ce ticket reprend seul à la clôture de {id} : rien à répondre.",
           "review_ask": "Réponds dans ton avis (section « Règles pour le code ») : {id} le lit et reprend seul.",
           "pr": "Pull request : {url}", "ticket": "Ticket {id}.",
           "reworked": "Repris à la demande du robot de fusion : {n} commit(s) poussé(s) sur {branch}, la pull request est à jour.",
           "reworked_none": "Repris à la demande du robot de fusion : rien de nouveau poussé sur {branch}.",
           "rework_failed": "Repris à la demande du robot de fusion, sans succès : {why}\n\nLa pull request reste ouverte pour une personne.",
           "refused": REFUSED["fr"] + " : {why}.\n\n"
                      "Les commits sont sur la branche {branch} du fork. L'agent réessaie seul au prochain passage, "
                      "autrement, sans ces fichiers (deux essais au plus) : rien à répondre.",
           "manual": "À fusionner par une personne, pas automatiquement : {why}.",
           "breach": {"files": "fichiers qu'il ne peut pas modifier : {v}", "binary": "fichiers binaires : {v}",
                      "lines": "{v} lignes modifiées, {max} au plus", "count": "{v} fichiers modifiés, {max} au plus",
                      "sql": "un changement destructif du schéma de la base : {v}",
                      "outside": "fichiers que le rôle {role} ne peut pas modifier : {v}"},
           "nothing": "le chef de projet n'a créé aucun sous-ticket. Son compte rendu :\n\n{report}",
           "closed": "Tous les sous-tickets sont clos :\n\n{list}", "states": {"done": "terminé", "cancelled": "annulé"}},
}
JOBS = queue.Queue()


class Refused(Exception):
    """Cairn Plan said no (ticket already taken, not approved, unknown)."""


class Breach(Exception):
    """The change breaks the rules of rules.json: no pull request."""


RECENT = collections.deque(maxlen=40)  # the last lines of the log, for /status
NOW = {"ticket": ""}  # the ticket in hand


def log(msg):
    print(msg, flush=True)
    RECENT.append({"at": int(time.time()), "text": msg})


def env(name, default=""):
    return os.environ.get(name, default).strip()


def secret(name):
    """The one line of the file named by an environment variable; unset: empty."""
    path = env(name)
    return open(path, encoding="utf-8").read().strip() if path else ""


def load():
    conf = {
        "listen": env("AGENT_LISTEN", "0.0.0.0:9095"),
        "work": env("AGENT_WORK", "/work"),
        "worker": env("AGENT_WORKER", "worker"),
        "hook_secret": secret("AGENT_HOOK_SECRET_FILE"),
        "mcp_url": env("PLAN_MCP_URL"),
        "mcp_key": secret("PLAN_MCP_KEY_FILE"),
        # one of the two: a subscription token (claude setup-token) or an API key
        "claude_env": {k: v for k, v in (("CLAUDE_CODE_OAUTH_TOKEN", secret("AGENT_CLAUDE_TOKEN_FILE")),
                                         ("ANTHROPIC_API_KEY", secret("AGENT_ANTHROPIC_KEY_FILE"))) if v},
        "git_user": env("AGENT_GIT_USER", "x-access-token"),
        "git_token": secret("AGENT_GIT_TOKEN_FILE"),
        "git_name": env("AGENT_GIT_NAME", "Cairn Plan agent"),
        "git_email": env("AGENT_GIT_EMAIL", "agent@localhost"),
        "repos": json.loads(env("AGENT_REPOS", "{}")),
        "daily_usd": float(env("AGENT_DAILY_USD", "20")),
        # the night window (local hours) and its factor of the budget: {} = one window a day
        "night": json.loads(env("AGENT_NIGHT", "{}")),
        "tz": env("AGENT_TZ", "Europe/Paris"),
        "model": env("AGENT_MODEL"),
        "max_turns": env("AGENT_MAX_TURNS", "150"),
        "timeout": int(env("AGENT_TIMEOUT_MIN", "60")) * 60,
        "lang": env("AGENT_LANG", "en") if env("AGENT_LANG", "en") in TEXT else "en",
        "sweep": int(env("AGENT_SWEEP_MIN", "60")) * 60,
        "rework_from": env("AGENT_REWORK_FROM"),
        "rules": json.load(open(env("AGENT_RULES_FILE", os.path.join(DIR, "rules.json")), encoding="utf-8")),
        "teams": teams(json.loads(env("AGENT_TEAMS", "{}")),
                       json.load(open(env("AGENT_TEAM_FILE", os.path.join(DIR, "team.json")), encoding="utf-8"))),
        "budgets": {},
    }
    missing = [k for k in ("hook_secret", "mcp_url", "mcp_key", "claude_env", "git_token", "repos") if not conf[k]]
    if missing:
        raise SystemExit(f"missing configuration: {', '.join(missing)} (see README.md)")
    if len(conf["hook_secret"]) < 16:
        raise SystemExit("the webhook secret is shorter than 16 characters")
    return conf


def teams(given, known):
    """The teams of AGENT_TEAMS, each role completed by team.json and its key read:
    {project: {"lead": name, "roles": {name: settings}}}."""
    out = {}
    for project, team in given.items():
        roles = {}
        for name, own in team["roles"].items():
            role = {**known["roles"].get(name, {}), **own}
            if role.get("note"):  # what only this project's team must know, after the role's own text
                role["text"] = f"{role.get('text', '')} {role['note']}".strip()
            # default: mcp-key-<role> next to the agent's own key
            path = role.get("key_file") or os.path.join(os.path.dirname(env("PLAN_MCP_KEY_FILE", "/run/agent/x")), f"mcp-key-{name}")
            role["key"] = open(path, encoding="utf-8").read().strip() if os.path.isfile(path) else ""
            if not role["key"]:
                raise SystemExit(f"team {project}: role {name} has no MCP key ({path})")
            roles[name] = role
        if team["lead"] not in roles:
            raise SystemExit(f"team {project}: the lead {team['lead']} is not one of its roles")
        out[project] = {"lead": team["lead"], "roles": roles}
    return out


# ---------- pure helpers (test_agent.py) ----------
def role_for(ticket, team):
    """The role that builds a ticket of a team: its "agent" field, else the lead."""
    return ticket.get("agent") or team["lead"]


def outside(paths, numstat):
    """Files of a change (`git diff --numstat -z`) under none of the paths of a role."""
    files = [rec.split("\t", 2)[2] for rec in numstat.split("\0") if rec]
    return [f for f in files if not any(matches(f, p) for p in paths)]


def waits_for(parent, tid):
    """The sub-tickets of the same parent, of another role, still open: a role that
    "waits" (the writer documents what the others built) starts after them."""
    kids = parent.get("children") or []
    me = next((k for k in kids if k["id"] == tid), {})
    return [k["id"] for k in kids if k["id"] != tid and k.get("agent") != me.get("agent")
            and k["state"] not in ("done", "cancelled")]


def reviews_for(parent, tid, team):
    """The review parts (roles marked "first": the lawyer, the DPO) of the same parent
    still open: every part of another role starts after them, their opinion to read.
    No team (the one agent serves the project): nothing to wait for."""
    if not team:
        return []
    roles = team["roles"]
    kids = parent.get("children") or []
    me = next((k for k in kids if k["id"] == tid), {})
    if roles.get(role_for(me, team), {}).get("first"):
        return []
    return [k["id"] for k in kids if k["id"] != tid and roles.get(role_for(k, team), {}).get("first")
            and k["state"] not in ("done", "cancelled")]


def blocked_tools(allowed):
    """Tools no role gets unless team.json grants them: a role may be given WebFetch on
    named domains only ("WebFetch(domain:...)"); any other address stays refused."""
    return [t for t in ("WebFetch", "WebSearch", "Agent", "NotebookEdit")
            if not any(a == t or a.startswith(t + "(") for a in allowed)]


TICKET_ID = re.compile(r"\b([A-Z][A-Z0-9]{1,9}-\d+)\b")


def project_of(tid):
    return tid.rsplit("-", 1)[0]


def answered(full):
    """Whoever approved the ticket spoke after its assignee's last word, whatever notes of others came after.
    A sub-ticket was approved by the lead, on the approval of its parent: either answers."""
    ap = full.get("approval") or {}
    by = {ap.get("by"), ((ap.get("parent") or {}).get("approval") or {}).get("by")} - {None, ""}
    for c in reversed(full.get("comments") or []):
        if c.get("author") in by:
            return True
        if c.get("author") == full.get("assignee"):
            return False
    return False


def nothing_to_build(report):
    """The lead's report ends with NOTHING alone on its last line: no sub-ticket is needed, the work is already done.
    The ticket is then closed with that report, instead of failing. Backticks or stars a model adds are ignored."""
    lines = [x.strip().strip("`*_ ") for x in (report or "").strip().splitlines()]
    return bool(lines) and lines[-1] == NOTHING


def rework_of(conf, full):
    """the merge bot's request to take a closed ticket again: its last comment, from the merge bot's account; {} if none."""
    last = (full.get("comments") or [{}])[-1]
    ok = full.get("state") == "done" and conf.get("rework_from") and last.get("author") == conf["rework_from"] \
        and last.get("body", "").startswith(REWORK)
    return last if ok else {}


def own_last(full):
    """The last comment of the ticket's assignee (its question, its wait, its failure), {} if none. A later note
    of someone else (a session that explains, a reader) does not undo the wake it calls for; an answer of
    whoever approved the ticket is caught first by the sweep."""
    me = full.get("assignee")
    return next((c for c in reversed(full.get("comments") or []) if me and c.get("author") == me), {})


def waited_parts(full):
    """The tickets of the same project that a waiting part named in its own last comment (its question: « I wait
    for the merge of SHOP-82 »), any parent, not itself nor its parent; [] when the last word is someone else's."""
    last = own_last(full)
    if not last:
        return []
    skip = {full["id"], full.get("parent") or ""}
    # only the sentences that speak of a merge (« j'attends la fusion de SHOP-81 », « fusionnez SHOP-82 »)
    said = [x for x in re.split(r"[.!?\n]", last.get("body", "")) if MERGE.search(x)]
    return sorted({i for x in said for i in TICKET_ID.findall(x) if project_of(i) == project_of(full["id"]) and i not in skip})


def own_prs(ticket):
    """The pull requests of the ticket itself. The forge links to a ticket every pull request that names it (PR 85 of
    SHOP-82 cites SHOP-80 and shows in SHOP-80): only « PR <n>: <ID>: … » (the agent's title) or the agent's own
    « PR: … » link count."""
    links = ticket.get("links") or []
    mine = re.compile(rf"^PR(?: \d+)?: (?:{re.escape(ticket.get('id', ''))}:|(?![A-Z][A-Z0-9]*-\d+:))")
    return {l["url"] for l in links if re.search(r"/pulls/\d+$", l.get("url", "")) and mine.match(l.get("title", ""))}


def open_prs(ticket):
    """The pull requests of a ticket still open: none of their links is marked « (merged) » or « (closed) »."""
    links = ticket.get("links") or []
    prs = own_prs(ticket)
    return [u for u in prs if not any(l["url"] == u and l.get("title", "").endswith(("(merged)", "(closed)")) for l in links)]


def merged(ticket):
    """A ticket closed as done whose every pull request of its own (link to .../pulls/<n>) is merged."""
    links = ticket.get("links") or []
    prs = own_prs(ticket)
    return ticket.get("state") == "done" and bool(prs) and all(
        any(l["url"] == u and l.get("title", "").endswith("(merged)") for l in links) for u in prs)


def settled(ticket):
    """Nothing more to wait for: cancelled, or done with no pull request still open (a closed one was given up
    or replaced, lest a part be held for ever)."""
    return ticket.get("state") == "cancelled" or (ticket.get("state") == "done" and not open_prs(ticket))


# « fais-le après SHOP-80 et SHOP-81 », « after BLOG-12 », « once … », « until … »: the ids of the rest of the
# sentence; « quand » / « when » only in a sentence that speaks of a merge (« commence quand SHOP-82 est fusionné »)
AFTER = re.compile(r"(?i)\b(?:après|apres|after|once|until|jusqu'à)\b([^.\n]*)")
WHEN = re.compile(r"(?i)\b(?:quand|when)\b([^.\n]*)")
MERGE = re.compile(r"(?i)fusion|merge")


def waited_in(description, tid, parent_id):
    named = {i for m in AFTER.finditer(description or "") for i in TICKET_ID.findall(m.group(1))}
    named |= {i for m in WHEN.finditer(description or "") if MERGE.search(m.group(1)) for i in TICKET_ID.findall(m.group(1))}
    return sorted(i for i in named if project_of(i) == project_of(tid) and i not in (tid, parent_id))


def unmerged_named(ticket, parent, fetch):
    """The tickets of the same project that a part's approved description says to wait for (« après SHOP-80 »,
    a part of the same parent or of another one) and that are not settled yet. Checked here, before Claude
    starts: no run just to say « not merged yet »."""
    kids = {k["id"]: k for k in (parent or {}).get("children") or []}
    held = []
    for i in waited_in(ticket.get("description"), ticket["id"], (parent or {}).get("id", "")):
        state = kids[i]["state"] if i in kids else None
        if state == "cancelled":
            continue
        if state not in (None, "done"):
            held.append(i)
            continue
        try:
            t = fetch(i)
        except Refused:  # not a ticket this account can read: nothing to wait for
            continue
        # an epic (a ticket with parts) stays open while its parts are built: never waited for, lest a part wait for ever
        if not t or t.get("children"):
            continue
        if not settled(t):
            held.append(i)
    return held


def limit_reset(text, now):
    """Unix time of the reset Claude announces (« resets 3:20pm (UTC) », « resets 9am (UTC) »), the next such
    moment after now; None when the message gives no time in UTC."""
    m = re.search(r"(?i)resets\s+(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)\s*\(UTC\)", text or "")
    if not m:
        return None
    h = int(m.group(1)) % 12 + (12 if m.group(3).lower() == "pm" else 0)
    t = datetime.datetime.fromtimestamp(now, datetime.timezone.utc).replace(hour=h, minute=int(m.group(2) or 0), second=0, microsecond=0)
    if t.timestamp() <= now:
        if now - t.timestamp() < 600:  # the reset just passed and Claude still says so: try again shortly, not tomorrow
            return int(now) + 300
        t += datetime.timedelta(days=1)
    return int(t.timestamp())


def pieces(text, limit=490_000):
    """A text in comments of at most `limit` bytes, cut between lines (inside a line only when it is longer than
    the limit), each marked (k/n) when there are several. Nothing is dropped."""
    parts, cur = [], ""
    for line in text.splitlines(keepends=True):
        while len(line.encode()) > limit:  # a line too long for one comment: cut it, on a character boundary
            head = line.encode()[:limit].decode(errors="ignore")
            if cur:
                parts.append(cur)
                cur = ""
            parts.append(head)
            line = line[len(head):]
        if len((cur + line).encode()) > limit:
            parts.append(cur)
            cur = ""
        cur += line
    if cur or not parts:
        parts.append(cur)
    if len(parts) == 1:
        return parts
    n = len(parts)
    return [f"({k}/{n})\n\n{part}" for k, part in enumerate(parts, 1)]


def summary(parent, words):
    """The closing report of a parent: its sub-tickets and how they ended."""
    return words["closed"].format(list="\n".join(
        f"- {k['id']} ({k.get('agent') or '-'}) : {k['title']}, {words['states'].get(k['state'], k['state'])}"
        for k in parent.get("children") or []))
def matches(path, pattern):
    """A pattern with a / against the whole path, one without against the file name."""
    return fnmatch.fnmatchcase(path if "/" in pattern else path.rsplit("/", 1)[-1], pattern)


# SQL of test code resets a test database, never production (shop PR 90, SHOP-110, 2026-11-27: a DROP SCHEMA
# of a test schema held the pull request for a person). Exempt only what the compiler leaves out of production: a Go
# `_test.go` file, and the lines of a Rust `#[cfg(test)]` + `mod tests {` module, up to its closing `}` at column 0,
# read in the file itself (show(path): its text at the head of the change), never in the hunk header.
GO_TEST = re.compile(r"_test\.go$")
HUNK = re.compile(r"^@@ -\d+(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


# ...and only when every object the line drops or empties is named for tests (rs_test_delete): a line text alone
# can be faked (a module in a raw string, a brace closed early), a DROP TABLE users never passes
TARGET = re.compile(r"(?i)\b(?:drop\s+(?:table|index|column|schema|database|view|trigger)(?:\s+if\s+exists)?|"
                    r"truncate(?:\s+table)?|alter\s+column)\s+([\"`\w.]+(?:\s*,\s*[\"`\w.]+)*)")


def test_object(line):
    names = [n for group in TARGET.findall(line) for n in group.split(",")]  # DROP TABLE a_test, users
    # « test » as a word between _ or . (rs_test_delete), not inside one (latest_scores, contests)
    return bool(names) and all(re.search(r"(^|[_.])tests?(_|$)", n.strip().strip("\"`").lower()) for n in names)


def test_lines(text):
    """Line numbers (1-based) of the `#[cfg(test)] mod tests { ... }` modules of a Rust file, and its lines."""
    out, lines, i = set(), (text or "").split("\n"), 0
    while i < len(lines):
        if lines[i].strip() == "#[cfg(test)]":
            j = i + 1
            while j < len(lines) and (not lines[j].strip() or lines[j].lstrip().startswith(("#[", "//"))):
                j += 1
            if j < len(lines) and re.match(r"mod tests\s*\{\s*$", lines[j]):
                k = j + 1
                while k < len(lines) and lines[k].rstrip() != "}":
                    k += 1
                if k < len(lines):  # an unclosed module exempts nothing
                    out.update(range(j + 2, k + 1))  # the lines inside the braces
                i = k
        i += 1
    return out, lines


def sql_hits(diff, sql, show=None, where=None):
    """The added lines of a `git diff -U0` that match `sql`, outside test code (their files appended to `where`, a list, when given). Hunks are read by their counts,
    so an added line that looks like a header stays an added line."""
    hits, path, new, olds, news, tests = [], "", 0, 0, 0, None
    for l in diff.splitlines():
        if olds or news:  # inside a hunk: exactly its counted lines
            if l.startswith("-"):
                olds -= 1
            elif l.startswith("+"):
                news -= 1
                if sql.search(l):
                    if tests is None:
                        tests = test_lines(show(path)) if show and path.endswith(".rs") else (set(), [])
                    inside, text = tests
                    # the same text at that line of the file, or no exemption (a shifted reading exempts nothing)
                    if not GO_TEST.search(path) and not (new in inside and text[new - 1].rstrip() == l[1:].rstrip() and test_object(l)):
                        hits.append(l[1:].strip()[:80])
                        if where is not None:
                            where.append(path)
                new += 1
            continue
        if l.startswith("+++ "):
            path, tests = (l[6:] if l.startswith("+++ b/") else l[4:]), None
        elif m := HUNK.match(l):
            olds, new, news = int(m.group(1) or 1), int(m.group(2)), int(m.group(3) if m.group(3) is not None else 1)
        elif l.startswith("+") and sql.search(l):  # a diff without headers: no exemption
            hits.append(l[1:].strip()[:80])
            if where is not None:
                where.append("")
    return hits


def breaches(rules, numstat, diff, show=None):
    """What rules.json forbids in a change, from `git diff --numstat -z
    --no-renames` and `git diff -U0` of it: {} when nothing."""
    found, files, lines = {}, 0, 0
    for rec in numstat.split("\0"):
        if not rec:
            continue
        added, deleted, path = rec.split("\t", 2)
        files += 1
        if added == "-":
            found.setdefault("binary", []).append(path)
        else:
            lines += int(added) + int(deleted)
        if any(matches(path, p) for p in rules["forbidden"]):
            found.setdefault("files", []).append(path)
    if lines > rules["max_lines"]:
        found["lines"] = lines
    if files > rules["max_files"]:
        found["count"] = files
    sql = re.compile(rules["forbidden_added"])
    hits = sql_hits(diff, sql, show)
    if hits:
        found["sql"] = hits[:3]
    return found


def blocking(found, rules):
    """The part of breaches() that stops the pull request itself (rules["blocking"]);
    the rest only keeps it from being merged automatically."""
    return [p for p in found.get("files", []) if any(matches(p, b) for b in rules.get("blocking", []))]


def asked(ticket, since):
    """Claude called wait_ticket during this run: a move to "waiting" since it started.
    Read from the history, not from the state, which someone may have changed since."""
    return any(e.get("field") == "state" and e.get("new") == "waiting" and e.get("at", 0) >= since
               for e in ticket.get("events", []))


def say_breaches(found, rules, words):
    limit = {"lines": rules["max_lines"], "count": rules["max_files"]}
    return "; ".join(words[k].format(v=", ".join(v) if isinstance(v, list) else v, max=limit.get(k))
                     for k, v in found.items())



def signed(secret_, body, header):
    """True if `header` ("sha256=<hex>") signs `body` under `secret_`."""
    good = "sha256=" + hmac.new(secret_.encode(), body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(good.encode(), (header or "").encode())


def accept(secret_, body, header):
    """What to answer a POST on /hook: (status, text, call to queue or None)."""
    if not signed(secret_, body, header):
        return 401, "bad signature", None
    try:
        call = json.loads(body)
    except ValueError:
        return 400, "invalid request", None
    if not isinstance(call, dict) or call.get("event") not in EVENTS:
        return 200, "pong" if isinstance(call, dict) and call.get("event") == "ping" else "ignored", None
    if not re.fullmatch(r"[A-Z][A-Z0-9]{1,9}-[0-9]{1,9}", str(call.get("ticket", ""))):
        return 400, "invalid request", None
    return 202, "queued", call


def forge(url):
    """(API address of the repository, owner, host) of an https clone address.
    github.com has its own API host; anything else is taken for Forgejo or
    Gitea, which serve the same calls under /api/v1."""
    u = urllib.parse.urlsplit(url)
    owner, name = u.path.strip("/").removesuffix(".git").split("/")[-2:]
    if u.scheme != "https" or not u.hostname:
        raise ValueError(f"not an https address: {url}")
    base = "https://api.github.com" if u.hostname == "github.com" else f"https://{u.netloc}/api/v1"
    return f"{base}/repos/{owner}/{name}", owner, u.netloc


def git_auth(urls, user, token):
    """Environment that gives git the token for the hosts of `urls` and for no
    other. In the environment, not on the command line: other users of the
    machine read command lines, not environments."""
    basic = base64.b64encode(f"{user}:{token}".encode()).decode()
    out, hosts = {}, sorted({urllib.parse.urlsplit(u).netloc for u in urls})
    for i, host in enumerate(hosts):
        out[f"GIT_CONFIG_KEY_{i}"] = f"http.https://{host}/.extraHeader"
        out[f"GIT_CONFIG_VALUE_{i}"] = f"Authorization: Basic {basic}"
    out["GIT_CONFIG_COUNT"] = str(len(hosts))
    return out


def pr_head(repo_url, push_url, branch):
    """The "head" of the pull request: "owner:branch" when pushed to a fork."""
    owner, pusher = forge(repo_url)[1], forge(push_url)[1]
    return branch if owner == pusher else f"{pusher}:{branch}"


def fill(template, ticket, parent=None, role="", roles=None):
    """The prompt: the ticket as JSON, request and data apart. A sub-ticket's
    request holds the approved parent it is part of: its frame."""
    request = {"id": ticket["id"], "title": ticket["title"], "description": ticket["description"]}
    if parent:
        request["part_of"] = {"id": parent["id"], "title": parent["title"], "description": parent["description"]}
    data = {k: ticket.get(k) for k in ("approval", "comments", "links", "files", "priority", "milestone")}
    if parent:
        data["other_parts"] = [k for k in parent.get("children") or [] if k["id"] != ticket["id"]]
    dump = lambda v: json.dumps(v, ensure_ascii=False, indent=2)
    return (template.replace("{{REQUEST}}", dump(request)).replace("{{DATA}}", dump(data)).replace("{{ID}}", ticket["id"])
            .replace("{{ROLE}}", role).replace("{{ROLES}}", dump(roles or {})))


class Budget:
    """Dollars spent in the current window of the day, kept in a file. Claude Code reports what a run would cost at
    API prices, subscription or not: the best measure of quota a headless run gives. Two windows (2026-11-27):
    by day the budget of the role, leaving room for the owner's own sessions; by night (`night`, local hours of `tz`)
    that budget times `factor`, the agent then stopping only on a usage limit or a 429. Each window starts from zero.
    No cap of tickets: the dollars are the one limit (a cap of 10 tickets stopped the lead with 5 $ left)."""

    clock = staticmethod(time.time)  # the tests set it

    def __init__(self, path, usd, night=None, tz="Europe/Paris"):
        self.path, self.usd, self.night, self.tz = path, usd, night or {}, tz

    def window(self):
        """(key, name, factor) of the window the clock is in; the night is keyed by the date it starts."""
        now = datetime.datetime.fromtimestamp(self.clock(), zoneinfo.ZoneInfo(self.tz))
        start, end = self.night.get("start"), self.night.get("end")
        if start is None or end is None or start == end:
            return now.date().isoformat(), "day", 1.0
        inside = (start <= now.hour or now.hour < end) if start > end else (start <= now.hour < end)
        if not inside:
            return f"{now.date().isoformat()}-day", "day", 1.0
        began = now.date() - datetime.timedelta(days=1) if start > end and now.hour < end else now.date()
        return f"{began.isoformat()}-night", "night", float(self.night.get("factor", 1))

    @property
    def max_usd(self):
        return self.usd * self.window()[2]

    def _read(self):
        key = self.window()[0]
        try:
            state = json.load(open(self.path, encoding="utf-8"))
        except (OSError, ValueError):
            state = {}
        if state.get("day") != key:
            # a new window starts from zero; a pause with a later end goes on (a limit reset after the change)
            keep = state.get("until", 0) > self.clock() and state.get("paused")
            state = {"day": key, "tickets": 0, "usd": 0.0, "paused": state["paused"] if keep else "",
                     "until": state.get("until", 0) if keep else 0}
        return state

    def _write(self, state):
        json.dump(state, open(self.path, "w", encoding="utf-8"))

    def refusal(self):
        """Why no ticket is taken right now; None: go ahead."""
        s = self._read()
        if s["paused"] and s.get("until") and self.clock() >= s["until"]:
            s.update(paused="", until=0)  # the limit was reset
            self._write(s)
        if s["paused"]:
            when = time.strftime("%H:%M UTC", time.gmtime(s["until"])) if s.get("until") else "the next window"
            return f"paused until {when} ({s['paused']})"
        if s["usd"] >= self.max_usd:
            return f"budget of {self.max_usd:g} $ reached for this {self.window()[1]} ({s['usd']:.2f} $ spent)"
        return None

    def state(self):
        """The window's count with its budget: what /status shows (tickets: how many were taken, no cap)."""
        s = self._read()
        return {"tickets": s["tickets"], "usd": round(s["usd"], 2), "max_usd": self.max_usd, "window": self.window()[1],
                "paused": s["paused"], "until": s.get("until", 0)}

    def add(self, tickets=0, usd=0.0, paused="", until=0):
        s = self._read()
        s.update(tickets=s["tickets"] + tickets, usd=s["usd"] + usd, paused=paused or s["paused"])
        if paused:
            s["until"] = until or 0
        self._write(s)


# ---------- the outside world ----------

def http_json(url, token, body=None):
    req = urllib.request.Request(url, data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Authorization": f"token {token}", "Content-Type": "application/json",
                                          "Accept": "application/json", "User-Agent": "cairn-plan-agent"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.load(resp)


def mcp(conf, tool, key=None, **args):
    """One tool of Cairn Plan's MCP server, under the account of `key` (default:
    the agent's); returns its answer, parsed.
    ponytail: plain JSON answers only, which is what Cairn Plan sends; read
    the event stream too if this ever talks to another MCP server."""
    body = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": tool, "arguments": args}}
    req = urllib.request.Request(conf["mcp_url"], data=json.dumps(body).encode(), headers={
        "Authorization": f"Bearer {key or conf['mcp_key']}", "Content-Type": "application/json", "Accept": "application/json"})
    # 429: the front end's rate limit (a sweep reads many tickets at once, 2026-11-27): wait, then again
    for wait in (2, 5, 15, 0):
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                out = json.load(resp)
            break
        except urllib.error.HTTPError as e:
            if e.code != 429 or not wait:
                raise
            after = str((e.headers or {}).get("Retry-After") or "")
            time.sleep(min(15, int(after)) if after.isdigit() else wait)  # 45 s of sleep at most per call
    result = out.get("result") or {}
    text = (result.get("content") or [{}])[0].get("text", "")
    if out.get("error") or result.get("isError"):
        raise Refused(text or json.dumps(out.get("error")))
    return json.loads(text)


def report_run(conf, key, run_id, **fields):
    """Tells Cairn Plan about a run of this agent (Agents page): a summary, never the
    instruction nor the output. A report that does not arrive is logged and the work goes on."""
    fields = {k: v for k, v in fields.items() if v not in (None, "", 0, 0.0)}
    try:
        mcp(conf, "report_run", key=key, run_id=run_id, source="cairn-plan-agent", host=env("AGENT_HOST", socket.gethostname()), **fields)
    except Exception as e:
        log(f"run {run_id} not reported: {e!r}")


def git(args, cwd=None, auth=None, check=True):
    """git as this program, in its own clone only."""
    p = subprocess.run(["git", *args], cwd=cwd, env={**os.environ, "GIT_TERMINAL_PROMPT": "0", **(auth or {})},
                       stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=900)
    if check and p.returncode:
        raise RuntimeError(f"git {args[0]}: {p.stderr.strip()[-400:]}")
    return p.stdout.strip() if check else p.returncode == 0


def as_worker(conf, cmd, cwd=None, extra=None, timeout=900):
    """A command as the unprivileged user, with none of this program's environment."""
    pw = pwd.getpwnam(conf["worker"])
    # HOME is in the work volume, not in the image: what Claude installs there (compilers, SDKs) is kept
    # from one ticket to the next. Claude Code itself stays where the image put it (PATH).
    base = {"HOME": os.path.join(conf["work"], "w", "home"), "PATH": f"{pw.pw_dir}/.local/bin:/usr/local/bin:/usr/bin:/bin", "LANG": "C.UTF-8",
            "GIT_AUTHOR_NAME": conf["git_name"], "GIT_AUTHOR_EMAIL": conf["git_email"],
            "GIT_COMMITTER_NAME": conf["git_name"], "GIT_COMMITTER_EMAIL": conf["git_email"]}
    # Output to files, not pipes: a process left in the background would hold
    # a pipe open and this program would wait for it instead of for the command.
    # In the work folder, which is this program's: /tmp belongs to the worker.
    with tempfile.TemporaryFile("w+", encoding="utf-8", errors="replace", dir=conf["work"]) as out, \
            tempfile.TemporaryFile("w+", encoding="utf-8", errors="replace", dir=conf["work"]) as err:
        p = subprocess.run(cmd, cwd=cwd or pw.pw_dir, env={**base, **(extra or {})}, user=pw.pw_uid, group=pw.pw_gid,
                           extra_groups=[], stdin=subprocess.DEVNULL, stdout=out, stderr=err, timeout=timeout)
        out.seek(0)
        err.seek(0)
        p.stdout, p.stderr = out.read(), err.read()
    return p


def wgit(conf, job, *args):
    p = as_worker(conf, ["git", *args], cwd=job)
    if p.returncode:
        raise RuntimeError(f"git {args[0]} (clone of the agent): {p.stderr.strip()[-400:]}")
    return p.stdout.strip()


def clean(conf):
    """Nothing of a ticket is left for the next: processes, clone, bundle, /tmp, the conversation.
    The tools installed in the home stay, while the volume has room."""
    w = os.path.join(conf["work"], "w")
    try:
        as_worker(conf, ["sh", "-c", "kill -9 -1"], timeout=30)  # every process of the worker, this one included
        # chmod: a Go module cache is read-only, rm alone leaves it
        as_worker(conf, ["sh", "-c", 'mkdir -p "$1"/home "$1"/tmp && chmod 1777 "$1"/tmp; '
                         'chmod -R u+w "$1"/tmp 2>/dev/null; find "$1"/tmp -mindepth 1 -delete; '
                         'rm -rf "$1"/job "$1"/out.bundle "$1"/home/.claude "$1"/home/.claude.json', "sh", w], timeout=300)
        if shutil.disk_usage(w).free < KEEP_FREE:
            log("clean: the work volume is nearly full, the tools of the home go")
            as_worker(conf, ["sh", "-c", 'chmod -R u+w "$1"/home; find "$1"/home -mindepth 1 -delete', "sh", w], timeout=600)
    except subprocess.SubprocessError as e:
        log(f"clean: {e}")
    shutil.rmtree(os.path.join(conf["work"], "src"), ignore_errors=True)
    if os.path.lexists(os.path.join(conf["work"], "in.bundle")):
        os.remove(os.path.join(conf["work"], "in.bundle"))


def open_pr(conf, repo, base, branch, title, body):
    api = forge(repo["repo"])[0]
    head = pr_head(repo["repo"], repo.get("push") or repo["repo"], branch)
    try:
        return http_json(f"{api}/pulls", conf["git_token"], {"head": head, "base": base, "title": title, "body": body})["html_url"]
    except urllib.error.HTTPError as e:
        if e.code not in (409, 422):
            raise RuntimeError(f"pull request refused by the forge: {e.code} {e.read()[:300].decode(errors='replace')}")
    # one is already open for this branch (the ticket was resumed): the push updated it
    for pr in http_json(f"{api}/pulls?state=open", conf["git_token"]):
        if pr["head"]["ref"] == branch:
            return pr["html_url"]
    raise RuntimeError("pull request refused by the forge, and none is open for the branch")


def claude(conf, job, prompt, role):
    """Runs Claude Code in the clone, with the account, model and tools of the role. Returns (report, dollars)."""
    # In a folder the worker cannot write. Written aside then renamed: the
    # one of the last ticket belongs to the worker and cannot be opened again.
    mcp_file = os.path.join(conf["work"], "mcp.json")
    if os.path.lexists(mcp_file + ".tmp"):
        os.remove(mcp_file + ".tmp")
    with open(mcp_file + ".tmp", "w", encoding="utf-8") as f:
        json.dump({"mcpServers": {"plan": {"type": "http", "url": conf["mcp_url"],
                                           "headers": {"Authorization": f"Bearer {role.get('key') or conf['mcp_key']}"}}}}, f)
    pw = pwd.getpwnam(conf["worker"])
    os.chmod(mcp_file + ".tmp", 0o400)  # before it changes hands: after, only its owner may
    os.chown(mcp_file + ".tmp", pw.pw_uid, pw.pw_gid)
    os.replace(mcp_file + ".tmp", mcp_file)
    allowed = role.get("allowed") or TOOLS
    cmd = ["claude", "-p", prompt, "--output-format", "json", "--max-turns", conf["max_turns"],
           "--mcp-config", mcp_file, "--strict-mcp-config", "--allowedTools", ",".join(allowed),
           "--disallowedTools", ",".join([*blocked_tools(allowed), *role.get("denied", [])])]
    if role.get("model") or conf["model"]:
        cmd += ["--model", role.get("model") or conf["model"]]
    try:
        p = as_worker(conf, cmd, cwd=job, timeout=conf["timeout"],
                      extra={**conf["claude_env"], "DISABLE_AUTOUPDATER": "1", "DISABLE_TELEMETRY": "1"})
    except subprocess.TimeoutExpired:
        raise RuntimeError(f"more than {conf['timeout'] // 60} minutes") from None
    try:
        out = json.loads(p.stdout)
    except ValueError:
        raise RuntimeError(f"Claude Code failed (code {p.returncode}): {(p.stderr or p.stdout).strip()[-400:]}") from None
    usd = float(out.get("total_cost_usd") or 0)
    if p.returncode or out.get("is_error") or not out.get("result"):
        err = RuntimeError(f"Claude Code: {str(out.get('result') or out.get('subtype') or p.stderr)[-400:]}")
        err.usd = usd
        raise err
    return out["result"], usd


def take_bundle(path, dest):
    """Copies the bundle the worker made to a file of this program: a plain
    file, never a link or a folder put there instead."""
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as src:
        if not stat.S_ISREG(os.fstat(src.fileno()).st_mode):
            raise RuntimeError("the bundle is not a plain file")
        with open(dest, "wb") as out:
            shutil.copyfileobj(src, out)


def orphan(ticket, name, role, kids=None):
    """A ticket "in progress" on the account of a role (claude-<role>, tasks/main.yml)
    while the agent holds none (the sweep runs between two tickets): a run lost to a
    restart. The lead's ticket stays in progress while its parts are built: an orphan
    only if the lead was stopped before it made any (`kids`: its sub-tickets)."""
    return bool(role) and ticket.get("assignee") == f"claude-{name}" and not (role.get("splits") and kids)


def question(ticket):
    """A question asked to the project owner (ask_owner: created "waiting").
    Once approved, what follows is rarely a change to the repository: it is
    left to the night routine and the sessions."""
    events = ticket.get("events") or []  # newest first
    return bool(events) and events[-1].get("field") == "created" and events[-1].get("new") == "waiting"


def budget_of(conf, name, usd):
    """The day's budget of a role (the one agent without a team: "agent", state.json)."""
    if name not in conf["budgets"]:
        file = "state.json" if name == "agent" else f"state-{name}.json"
        conf["budgets"][name] = Budget(os.path.join(conf["work"], file), usd, conf.get("night"), conf.get("tz", "Europe/Paris"))
    return conf["budgets"][name]


def close_parent(conf, team, parent_id, words, closed_id=None):
    """After a sub-ticket: queue the parts still to do (a role that waited may go now),
    and close the parent, under the lead's account, once every part is closed."""
    lead = team["roles"][team["lead"]]["key"]
    parent = mcp(conf, "get_ticket", key=lead, id=parent_id)
    if parent["state"] != "in_progress":
        return
    left = [k for k in parent.get("children") or [] if k["state"] not in ("done", "cancelled")]
    for k in left:
        role = team["roles"].get(role_for(k, team)) or {}
        job = {"event": "ticket.approved", "ticket": k["id"]}
        if closed_id:  # the log then says which closing woke the part
            job["woken_by"] = f"ticket.closed {closed_id}"
        # a role that waits goes once nothing it waits for is open; never twice in the queue
        queued = [j.get("ticket") for j in list(JOBS.queue)]
        if k["state"] == "todo" and not (role.get("waits") and waits_for(parent, k["id"])) \
                and not reviews_for(parent, k["id"], team) and k["id"] not in queued:
            JOBS.put(job)
        elif k["state"] == "waiting" and closed_id and k["id"] not in queued and not reviews_for(parent, k["id"], team) \
                and forwarded(mcp(conf, "get_ticket", key=lead, id=k["id"])):
            JOBS.put({"event": "ticket.answered", "ticket": k["id"], "woken_by": f"ticket.closed {closed_id}"})
    if not left:
        mcp(conf, "finish_ticket", key=lead, id=parent_id, report=summary(parent, words))
        log(f"[{team['lead']}] {parent_id}: every sub-ticket is closed, done")


def closed(conf, call):
    """A sub-ticket was closed, by an agent or a person (ticket.closed): what follows
    it under the same parent is queued, and the parent closed once every part is."""
    tid = call["ticket"]
    team = conf["teams"].get(tid.rsplit("-", 1)[0])
    if not team:
        return
    kid = mcp(conf, "get_ticket", key=team["roles"][team["lead"]]["key"], id=tid)
    if kid.get("parent"):
        close_parent(conf, team, kid["parent"], TEXT[conf["lang"]], tid)


def work(conf, call, templates):
    if call.get("event") == "ticket.closed":
        return closed(conf, call)
    tid = call["ticket"]
    project = tid.rsplit("-", 1)[0]  # from the id, not from what else the call says
    repo = conf["repos"].get(project)
    if not repo:
        return log(f"{tid}: no repository is set for project {project}, left alone")
    team = conf["teams"].get(project)
    try:
        # read under the lead's account in a team (its accounts are the members), else the agent's
        first = mcp(conf, "get_ticket", key=team and team["roles"][team["lead"]]["key"], id=tid)
    except Refused as e:
        return log(f"{tid}: not read: {e}")
    if question(first):
        return log(f"{tid}: a question to the owner, left to the routine")
    if first.get("agent") == ROUTINE:
        return log(f"{tid}: marked for the routine, left alone")
    # read again here, never trusted from the call: the merge bot's comment, last in the closed ticket
    rework = call.get("event") == "ticket.rework"
    wanted = rework_of(conf, first) if rework else {}
    if rework and not wanted:
        return log(f"{tid}: no request of the merge bot to take it again, left alone")
    mark = f"{tid}:rework:{wanted.get('at')}"
    # a request that did not start goes back to the sweep (marked once when it was queued)
    unmark = lambda: woken_file(conf, lambda w: w.pop(mark, None)) if rework else None
    # the role is read in the ticket, never in the call
    name = role_for(first, team) if team else "agent"
    role = team["roles"].get(name) if team else {}
    if role is None:
        unmark()
        return log(f"{tid}: role {name} is not in the team of {project}, left alone")
    say = lambda m: log(f"[{name}] {tid}: {m}")
    key = role.get("key")
    budget = budget_of(conf, name, float(role.get("usd", conf["daily_usd"])))
    why = budget.refusal()
    if why:
        unmark()
        return say(f"left alone, {why}")
    try:
        parent = mcp(conf, "get_ticket", key=key, id=first["parent"]) if first.get("parent") else None
        if rework:  # closed: it waits for nothing, and Cairn Plan would refuse to take it ("done")
            ticket = first
        else:
            if role.get("waits") and parent and waits_for(parent, tid):
                return say(f"waits for {', '.join(waits_for(parent, tid))}")
            if parent and reviews_for(parent, tid, team):
                return say(f"waits for the review {', '.join(reviews_for(parent, tid, team))}")
            held = unmerged_named(first, parent, lambda i: mcp(conf, "get_ticket", key=key, id=i)) if parent else []
            if held:  # the hourly sweep queues it again; it goes once they are merged
                return say(f"waits for the merge of {', '.join(held)}")
            ticket = mcp(conf, "take_ticket", key=key, id=tid)
    except Refused as e:
        unmark()
        return say(f"not taken: {e}")
    except Exception:  # Cairn Plan unreachable: the next sweep tries again
        unmark()
        raise
    say(f"taken ({call.get('woken_by') or call['event']})")
    if call.get("woken_by") == RETRY:
        woken_file(conf, lambda w: w.__setitem__(f"{tid}:retry", w.get(f"{tid}:retry", 0) + 1))
    NOW["ticket"] = tid
    template = templates[role.get("prompt", "prompt.md")]
    others = {n: r.get("text", "") for n, r in (team or {}).get("roles", {}).items() if n != name}
    started, text = time.time(), TEXT[conf["lang"]]
    # one run per turn of work on the ticket: reported now, and again when it ends
    run_id, usd = f"plan-agent-{tid}-{int(started)}", 0.0
    run = dict(agent=name, task=ticket["title"][:120], ticket=tid, model=role.get("model") or conf["model"], started=int(started))
    report_run(conf, key, run_id, state="running", **run)
    w = os.path.join(conf["work"], "w")
    src, job, bundle, branch = os.path.join(conf["work"], "src"), os.path.join(w, "job"), os.path.join(w, "out.bundle"), f"agent/{tid}"
    push = repo.get("push") or repo["repo"]
    try:
        auth = git_auth([repo["repo"], push], conf["git_user"], conf["git_token"])
        budget.add(tickets=1)
        clean(conf)
        git(["clone", "-q", *(["--branch", repo["base"]] if repo.get("base") else []), repo["repo"], src], auth=auth)
        base = git(["rev-parse", "--abbrev-ref", "HEAD"], cwd=src)
        # a ticket taken again goes on from its branch
        resumed = git(["fetch", "-q", push, f"{branch}:{branch}"], cwd=src, auth=auth, check=False)
        # the branches of the other parts, to read (origin/agent/<id> in the clone)
        for k in (parent or {}).get("children") or []:
            if k["id"] != tid:
                git(["fetch", "-q", push, f"agent/{k['id']}:agent/{k['id']}"], cwd=src, auth=auth, check=False)
        wgit(conf, w, "clone", "-q", "--no-hardlinks", src, job)
        wgit(conf, job, "checkout", "-q", "-b", branch, f"origin/{branch if resumed else base}")
        prompt = fill(template, ticket, parent, ("Your role in the team: " + role["text"]) if role.get("text") else "", others)
        conflicted = []
        if rework:
            if not resumed:
                raise RuntimeError(f"branch {branch} is not in the fork any more")
            # main merged first, by this program: a clean merge needs no Claude for it; a conflict is left for Claude
            m = as_worker(conf, ["git", "merge", "--no-edit", f"origin/{base}"], cwd=job)
            conflicted = [f for f in wgit(conf, job, "diff", "--name-only", "-z", "--diff-filter=U").split("\0") if f] \
                if m.returncode else []
            if m.returncode and not conflicted:
                raise RuntimeError(f"git merge origin/{base}: {m.stderr.strip()[-400:]}")
            prompt += REWORK_PROMPT.format(ask=wanted.get("body", ""), merge=(
                MERGE_CONFLICT.format(base=base, files=", ".join(conflicted)) if conflicted else MERGE_DONE.format(base=base)))
        try:
            report, usd = claude(conf, job, prompt, role)
        except RuntimeError as e:
            budget.add(usd=getattr(e, "usd", 0.0))
            if LIMIT.search(str(e)):
                # this role only, its own budget; until the reset Claude gives, else until tomorrow
                budget.add(paused="Claude said its limit is reached", until=limit_reset(str(e), time.time()) or 0)
                e.limited = True
            raise
        budget.add(usd=usd)
        # nothing Claude left running may touch what comes next
        as_worker(conf, ["sh", "-c", "kill -9 -1"], timeout=30)
        if conflicted:  # never a conflict marker committed: the run fails instead (grep as the worker: its files)
            # from the output, not the exit code: a conflicted file Claude deleted makes grep exit 2
            left = as_worker(conf, ["grep", "-lsE", "^(<<<<<<<|>>>>>>>)( |$)", "--", *conflicted], cwd=job).stdout.strip()
            if left:
                raise RuntimeError(f"conflict markers left in: {', '.join(left.splitlines())}")
        # what Claude left uncommitted is part of the work (and ends a merge left open)
        as_worker(conf, ["git", "add", "-A", "--", ".", ":(exclude,glob)**/core.[0-9]*"], cwd=job)
        as_worker(conf, ["git", "commit", "-q", "-m", f"{tid}: {ticket['title']}"], cwd=job)
        ahead = int(wgit(conf, job, "rev-list", "--count", f"origin/{base}..HEAD"))
        new = int(wgit(conf, job, "rev-list", "--count", f"origin/{branch if resumed else base}..HEAD"))
        if new:
            wgit(conf, job, "bundle", "create", "-q", bundle, f"origin/{base}..{branch}")
            mine = os.path.join(conf["work"], "in.bundle")
            take_bundle(bundle, mine)
            git(["-c", "transfer.fsckObjects=true", "fetch", "-q", mine, f"{branch}:{branch}"], cwd=src)
            git(["push", "-q", push, f"{branch}:{branch}"], cwd=src, auth=auth)
        now = mcp(conf, "get_ticket", key=key, id=tid)
        if not rework and (now["state"] != "in_progress" or asked(now, int(started))):  # Claude asked a question
            report_run(conf, key, run_id, state="waiting", cost_usd=usd)
            sent = forward(conf, team, name, now, parent) if team and parent else ""
            return say(f"now {now['state']}, question asked{' to ' + sent if sent else ''}, {new} commit(s) pushed, {usd:.2f} $")
        # a comment holds 500 000 bytes (Cairn Plan MAX_COMMENT); past it, several comments, never cut
        # (SHOP-96, 2026-11-26: the DPO's « Règles pour le code » were lost past 9 000 bytes)
        if ahead:
            span = f"origin/{base}...{branch}"
            numstat = git(["diff", "--numstat", "-z", "--no-renames", span], cwd=src)
            # the size of each pull request, in the log: is the split small enough (2026-11-27, max_lines)
            size = sum(int(a) + int(d) for a, d, *_ in (r.split("\t", 2) for r in numstat.split("\0") if r) if a != "-")
            # the paths of the role: checked here, out of Claude's reach (the forge grants a whole repository)
            off = outside(role["paths"], numstat) if "paths" in role else []
            if off:
                raise Breach(text["breach"]["outside"].format(role=name, v=", ".join(off[:20])))
            found = breaches(conf["rules"], numstat,
                             git(["diff", "-U0", "--no-renames", "--no-color", "--no-ext-diff", span], cwd=src),
                             lambda p: git(["show", f"{branch}:{p}"], cwd=src))
            stop = blocking(found, conf["rules"])
            if stop:
                raise Breach(say_breaches({"files": stop}, conf["rules"], text["breach"]))
            if found:  # the pull request is opened all the same; whoever merges reads rules.json again
                report = text["manual"].format(why=say_breaches(found, conf["rules"], text["breach"])) + "\n\n" + report
            body = f"{text['ticket'].format(id=tid)}\n\n{report.encode()[:50000].decode(errors='ignore')}"
            url = open_pr(conf, repo, base, branch, f"{tid}: {ticket['title']}", body)
            if not rework:  # taken again: the open pull request, already linked
                mcp(conf, "add_link", key=key, id=tid, url=url, title=f"PR: {ticket['title']}"[:200])
            report += "\n\n" + text["pr"].format(url=url)
        cost = f"\n\n{name} · {role.get('model') or conf['model'] or '-'} · {usd:.2f} $"
        if rework:  # the ticket stays closed: the report is a comment, after the merge bot's (so not taken again for it)
            # the ticket's own commits, not those of the base brought by the merge
            own = int(wgit(conf, job, "rev-list", "--count", f"origin/{branch}..HEAD", f"^origin/{base}"))
            words = text["reworked" if new else "reworked_none"].format(n=own, branch=branch)
            for part in pieces(words + "\n\n" + report + cost):
                mcp(conf, "comment_ticket", key=key, id=tid, body=part)
            report_run(conf, key, run_id, state="ok", cost_usd=usd)
            return say(f"taken again for the merge bot, {own} commit(s) pushed, {usd:.2f} $")
        parts = now.get("children") or []
        if role.get("splits") and not parts and not nothing_to_build(report):
            raise RuntimeError(text["nothing"].format(report=report[:4000]))
        if role.get("splits") and parts:
            # the lead splits and builds nothing: the parent stays in progress until its parts are closed
            for part in pieces(report + cost):
                mcp(conf, "comment_ticket", key=key, id=tid, body=part)
            report_run(conf, key, run_id, state="ok", cost_usd=usd)
            woken_file(conf, lambda w: w.pop(f"{tid}:retry", None))
            return say(f"split into {', '.join(k['id'] for k in parts)}, {usd:.2f} $")
        *first, last = pieces(report + cost)
        for part in first:  # the beginning of a long report, in order, before the closing one
            mcp(conf, "comment_ticket", key=key, id=tid, body=part)
        mcp(conf, "finish_ticket", key=key, id=tid, report=last, minutes=max(1, round((time.time() - started) / 60)))
        report_run(conf, key, run_id, state="ok", cost_usd=usd)
        say(f"done, {ahead} commit(s), {size} changed line(s), {usd:.2f} $" if ahead else f"done, 0 commit(s), {usd:.2f} $")
        woken_file(conf, lambda w: w.pop(f"{tid}:retry", None))  # a later failure gets its tries again
        if team and parent:
            try:
                close_parent(conf, team, parent["id"], text)
            except Exception as e:
                say(f"parent {parent['id']} not updated: {e!r}")
    except Exception as e:  # whatever went wrong, the ticket must not stay "in progress" with nobody on it
        say(f"failed: {e!r}")
        report_run(conf, key, run_id, state="failed", cost_usd=usd or getattr(e, "usd", 0.0))
        why = str(e)[:1500]
        if rework:  # closed: no wait_ticket. The sweep tries again RETRIES times (the merge bot's comment stays last)
            # a usage limit is not a try: the sweep waits for the budget of the role
            tries = woken_file(conf).get(mark + ":failed", 0) + (0 if getattr(e, "limited", False) else 1)
            if tries <= RETRIES:
                woken_file(conf, lambda w: (w.pop(mark, None), w.__setitem__(mark + ":failed", tries)))
                return say(f"taken again for the merge bot, failed ({tries}/{RETRIES}), the sweep tries again")
            try:
                mcp(conf, "comment_ticket", key=key, id=tid, body=text["rework_failed"].format(why=why))
            except Exception as e2:
                say(f"could not even say so in the ticket: {e2!r}")
            return
        try:
            if isinstance(e, Breach):
                ask = text["refused"].format(why=why, branch=branch)
            elif getattr(e, "limited", False):
                until = budget.state().get("until") or 0
                when = (" (" + time.strftime("%H:%M UTC", time.gmtime(until)) + ")") if until else ""
                ask = text["limited"].format(why=why, when=when)
            else:
                ask = text["failed"].format(why=why)
            mcp(conf, "wait_ticket", key=key, id=tid, question=ask)
        except Exception as e2:
            say(f"could not even say so in the ticket: {e2!r}")
    finally:
        NOW["ticket"] = ""
        clean(conf)


def sweep(conf):
    """Queues what a lost call left behind (the container was down, the day's
    cap was reached): approved tickets nobody is assigned to, and waiting
    tickets whose last comment is from whoever approved them, and tickets a
    restart left "in progress" on a role's account. A ticket assigned to
    someone else is theirs and is left alone."""
    today = datetime.datetime.now(datetime.timezone.utc).date().isoformat()  # Cairn Plan wakes on the UTC day
    for project in sorted(conf["repos"]):
        team = conf.get("teams", {}).get(project)
        try:
            # take_ticket refuses "in progress": the role says so in the ticket ("waiting"), then takes it again
            for t in mcp(conf, "list_tickets", project=project, state="in_progress")["tickets"] if team else []:
                name = role_for(t, team)
                role = team["roles"].get(name)
                kids = role and role.get("splits") and mcp(conf, "get_ticket", key=role["key"], id=t["id"]).get("children")
                if orphan(t, name, role, kids):
                    # out of budget today: left in progress, the next sweep tries again
                    if budget_of(conf, name, float(role.get("usd", conf["daily_usd"]))).refusal():
                        continue
                    try:
                        mcp(conf, "wait_ticket", key=role["key"], id=t["id"], question=TEXT[conf["lang"]]["lost"])
                    except Refused as e:  # moved by someone since the list was read
                        log(f"[{name}] {t['id']}: in progress with no run, not taken again: {e}")
                        continue
                    log(f"[{name}] {t['id']}: in progress with no run (agent restarted), queued again")
                    JOBS.put({"event": "ticket.resumed", "ticket": t["id"]})
            for t in mcp(conf, "list_tickets", project=project)["tickets"]:
                # a ticket that sleeps until a later day is left: Cairn Plan calls when it wakes
                if not t.get("assignee") and t.get("agent") != ROUTINE and not t.get("wake_date", "") > today:
                    JOBS.put({"event": "ticket.approved", "ticket": t["id"]})
            # closed tickets the merge bot sends back (écarts, conflict): only those changed lately (a comment bumps "updated")
            for t in mcp(conf, "list_tickets", project=project, state="done")["tickets"]:
                if t.get("updated", 0) < time.time() - REWORK_DAYS * 86400 or t.get("agent") == ROUTINE:
                    continue
                full = mcp(conf, "get_ticket", id=t["id"])
                wanted = rework_of(conf, full)
                name = role_for(full, team) if team else "agent"
                role = (team or {}).get("roles", {}).get(name, {}) if team else {}
                if not wanted or budget_of(conf, name, float(role.get("usd", conf["daily_usd"]))).refusal():
                    continue
                if mark_once(conf, f"{t['id']}:rework:{wanted.get('at')}"):
                    log(f"[{name}] {t['id']}: sent back by the merge bot, queued again")
                    JOBS.put({"event": "ticket.rework", "ticket": t["id"]})
            for t in mcp(conf, "list_tickets", project=project, state="waiting")["tickets"]:
                full = mcp(conf, "get_ticket", id=t["id"])
                if answered(full):
                    JOBS.put({"event": "ticket.answered", "ticket": t["id"]})
                elif limited_again(conf, team, full):
                    log(f"[{role_for(full, team) if team else 'agent'}] {t['id']}: limit reset, queued again")
                    JOBS.put({"event": "ticket.answered", "ticket": t["id"], "woken_by": "limit reset"})
                elif failed_again(conf, team, full):
                    log(f"[{role_for(full, team) if team else 'agent'}] {t['id']}: failed run, tried again")
                    JOBS.put({"event": "ticket.answered", "ticket": t["id"], "woken_by": RETRY})
                elif team and full.get("parent"):
                    try:
                        if wake_if_reviewed(conf, team, full):
                            log(f"[{role_for(full, team)}] {t['id']}: no review open any more, queued again")
                            JOBS.put({"event": "ticket.answered", "ticket": t["id"], "woken_by": "review closed"})
                        else:
                            wake_if_merged(conf, team, full)
                    except Exception as e:  # one ticket never stops the sweep of the others
                        log(f"{t['id']}: merge check: {e!r}")
        except Exception as e:  # Cairn Plan not up yet, a project the account is not a member of
            log(f"sweep {project}: {e!r}")


def limited_again(conf, team, full):
    """A ticket whose last word is the agent's own « usage limit » comment, and whose role may work again."""
    last = own_last(full)
    if not last:
        return False
    if not any(last.get("body", "").startswith(m) for m in LIMITED.values()):
        return False
    name = role_for(full, team) if team else "agent"
    role = (team or {}).get("roles", {}).get(name, {}) if team else {}
    return budget_of(conf, name, float(role.get("usd", conf["daily_usd"]))).refusal() is None


def forwarded(full):
    """The last comment of a ticket is its assignee's own « question sent to a review role » (forward)."""
    last = own_last(full)
    return bool(last) and any(last.get("body", "").startswith(m) for m in FORWARDED.values())


def wake_if_reviewed(conf, team, full):
    """A forwarded part whose parent has no open review any more goes again, once per question (woken.json):
    the sweep catches what a lost ticket.closed, or a second review of the same parent, left waiting."""
    if not forwarded(full):
        return False
    parent = mcp(conf, "get_ticket", key=team["roles"][team["lead"]]["key"], id=full["parent"])
    if reviews_for(parent, full["id"], team):
        return False
    return mark_once(conf, f"{full['id']}:fwd:{own_last(full).get('at')}")


def woken_file(conf, change=None):
    """woken.json: read it, or apply `change` to it and write it back."""
    path = os.path.join(conf["work"], "woken.json")
    try:
        woken = json.load(open(path, encoding="utf-8"))
    except (OSError, ValueError):
        woken = {}
    if change:
        change(woken)
        with open(path + ".tmp", "w", encoding="utf-8") as f:
            json.dump(woken, f)
        os.replace(path + ".tmp", path)
    return woken


def mark_once(conf, mark):
    if mark in woken_file(conf):
        return False
    woken_file(conf, lambda w: w.__setitem__(mark, 1))
    return True


def forward(conf, team, name, ticket, parent):
    """A question of a building role that starts with a review role (« dpo : … », « juriste : … ») goes to that
    role, not to the owner: a review sub-ticket of the same parent, created by the lead. The part waits for it
    (reviews_for) and goes again once it is closed (close_parent). Returns "<role> (<id>)", or ""."""
    reviews = [n for n, r in team["roles"].items() if r.get("first")]
    me = team["roles"].get(name) or {}
    if not reviews or me.get("first") or me.get("splits"):
        return ""
    last = (ticket.get("comments") or [{}])[-1]
    m = re.match(rf"\W*({'|'.join(map(re.escape, reviews))})\W*:\s*(\S.*)", last.get("body", ""), re.I | re.S)
    if not m or last.get("author") != ticket.get("assignee"):
        return ""
    role, body = next(n for n in reviews if n.lower() == m.group(1).lower()), m.group(2).strip()
    lead = team["roles"][team["lead"]]["key"]
    try:
        kid = mcp(conf, "create_subticket", key=lead, parent=parent["id"], role=role,
                  title=f"Question de {ticket['id']} : {body.splitlines()[0]}"[:200],
                  description=f"{body}\n\n" + TEXT[conf["lang"]]["review_ask"].format(id=ticket["id"]))
        mcp(conf, "comment_ticket", key=me["key"], id=ticket["id"],
            body=TEXT[conf["lang"]]["forwarded"].format(role=role, id=kid["id"]))
    except Exception as e:  # 8 parts at most, parent closed: the question stays in the ticket
        log(f"[{name}] {ticket['id']}: question not sent to {role}: {e!r}")
        return ""
    return f"{role} ({kid['id']})"


def failed_again(conf, team, full):
    """A ticket whose last word is the agent's own failure or refusal comment: tried again RETRIES times at most
    (woken.json), when its role may work. A technical failure is the agent's to fix, never a question to the owner."""
    last = own_last(full)
    if not last:
        return False
    if not any(last.get("body", "").startswith(m) for m in (*FAILED.values(), *REFUSED.values())):
        return False
    name = role_for(full, team) if team else "agent"
    role = (team or {}).get("roles", {}).get(name, {}) if team else {}
    if budget_of(conf, name, float(role.get("usd", conf["daily_usd"]))).refusal():
        return False
    # counted by work() once the ticket is really taken again (a part held by a review spends no try)
    return woken_file(conf).get(f"{full['id']}:retry", 0) < RETRIES


def wake_if_merged(conf, team, full):
    """A part that waits for the merge of other tickets of its project (named in its own question) goes
    again once their pull requests are merged: nobody has to say "go". Once per ticket and parts (woken.json),
    so that the same wait asked again is not answered by a loop."""
    lead = team["roles"][team["lead"]]["key"]
    waited = waited_parts(full)
    if not waited:
        return
    named = []
    for i in waited:
        try:
            t = mcp(conf, "get_ticket", key=lead, id=i)
            if t and not t.get("children"):  # an epic is not something to merge
                named.append(t)
        except Refused:  # an id that is no ticket of this account (a typo, another space): not waited for
            pass
    # each named ticket settled, and at least one merged: the wait it spoke of is over
    if not all(settled(t) for t in named) or not any(merged(t) for t in named):
        return
    path = os.path.join(conf["work"], "woken.json")
    try:
        woken = json.load(open(path, encoding="utf-8"))
    except (OSError, ValueError):
        woken = {}
    # once per ticket and set of parts: the same wait asked again after its wake goes to a person, not to a loop
    mark = f"{full['id']}:{','.join(waited)}"
    if mark in woken:
        return
    woken[mark] = own_last(full).get("at")
    with open(path + ".tmp", "w", encoding="utf-8") as f:
        json.dump(woken, f)
    os.replace(path + ".tmp", path)
    log(f"[{role_for(full, team)}] {full['id']}: {', '.join(waited)} merged, queued again")
    JOBS.put({"event": "ticket.answered", "ticket": full["id"], "woken_by": f"merged {', '.join(waited)}"})


def status(conf):
    """What the agent is doing, for the page of Cairn Plan: the ticket in
    hand, what waits, the day's budget of each role, the last lines of the log."""
    roles = {"agent": float(conf["daily_usd"])}
    for team in conf["teams"].values():
        for name, role in team["roles"].items():
            roles[name] = float(role.get("usd", conf["daily_usd"]))
    return {"now": int(time.time()), "ticket": NOW["ticket"], "queued": JOBS.qsize(), "projects": sorted(conf["repos"]),
            "budgets": [dict(budget_of(conf, name, usd).state(), role=name) for name, usd in sorted(roles.items())],
            "recent": list(RECENT)[::-1]}


def serve(conf):
    class Hook(http.server.BaseHTTPRequestHandler):
        def answer(self, code, text):
            self.send_response(code)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(text.encode())

        def do_GET(self):
            if self.path == "/status":  # for Cairn Plan only: it knows the secret of the webhook
                given = (self.headers.get("Authorization") or "").encode()
                if not conf["hook_secret"] or not hmac.compare_digest(given, ("Bearer " + conf["hook_secret"]).encode()):
                    return self.answer(401, "unauthorized")
                return self.answer(200, json.dumps(status(conf)))
            self.answer(*((200, "ok") if self.path == "/healthz" else (404, "not found")))

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            if self.path != "/hook" or not 0 < length <= 100_000:
                return self.answer(404, "not found")
            code, text, call = accept(conf["hook_secret"], self.rfile.read(length), self.headers.get("X-Hub-Signature-256"))
            if call:
                JOBS.put(call)
            self.answer(code, text)

        def log_message(self, *_):
            pass

    host, port = conf["listen"].rsplit(":", 1)
    http.server.ThreadingHTTPServer((host, int(port)), Hook).serve_forever()


def main():
    conf = load()
    w, pw = os.path.join(conf["work"], "w"), pwd.getpwnam(conf["worker"])
    if not os.path.isdir(w):  # the one folder of the worker; created before it ever runs
        os.makedirs(w)
        os.chown(w, pw.pw_uid, pw.pw_gid)
    # The home of the worker, and /tmp (a link to w/tmp in the image). Made as the worker: this program
    # cannot write in w. clean() makes them again if a ticket removed them.
    subprocess.run(["sh", "-c", 'mkdir -p "$1"/home "$1"/tmp && chmod 1777 "$1"/tmp', "sh", w],
                   user=pw.pw_uid, group=pw.pw_gid, extra_groups=[], check=True)
    names = {"prompt.md"} | {r.get("prompt", "prompt.md") for t in conf["teams"].values() for r in t["roles"].values()}
    templates = {n: open(os.path.join(DIR, n), encoding="utf-8").read() for n in names}
    threading.Thread(target=serve, args=(conf,), daemon=True).start()
    crews = ["team %s: %s" % (p, ", ".join("%s (%s)" % (n, r.get("model", "-")) for n, r in t["roles"].items()))
             for p, t in sorted(conf["teams"].items())]
    log(f"cairn-plan-agent listening on {conf['listen']}, projects: {', '.join(sorted(conf['repos']))}"
        + "".join("; " + c for c in crews))
    sweep(conf)
    while True:  # one ticket at a time
        try:
            call = JOBS.get(timeout=conf["sweep"] or None)
        except queue.Empty:  # nothing came for a while: look for what was missed
            sweep(conf)
            continue
        try:
            work(conf, call, templates)
        except Exception as e:
            log(f"{call.get('ticket')}: {e!r}")


if __name__ == "__main__":
    main()

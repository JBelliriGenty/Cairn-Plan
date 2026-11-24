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

DIR = os.path.dirname(os.path.abspath(__file__))
EVENTS = ("ticket.approved", "ticket.answered", "ticket.closed")
# What Claude may use. Taking and closing the ticket stay with this program.
TOOLS = ["Read", "Edit", "Write", "Glob", "Grep", "Bash",
         "mcp__plan__get_ticket", "mcp__plan__comment_ticket", "mcp__plan__wait_ticket", "mcp__plan__propose_ticket"]
TEXT = {
    "en": {"failed": "The agent stopped before the end: {why}\n\nAnswer this comment to have it try again.",
           "pr": "Pull request: {url}", "ticket": "Ticket {id}.",
           "refused": "The work is done, but the rules of the agent forbid opening this pull request: {why}.\n\n"
                      "The commits are on branch {branch} of the fork. Answer this comment (for instance with "
                      "another way to do it) to have it try again, or open the pull request by hand.",
           "manual": "To be merged by a person, not automatically: {why}.",
           "breach": {"files": "files it may not change: {v}", "binary": "binary files: {v}",
                      "lines": "{v} lines changed, at most {max}", "count": "{v} files changed, at most {max}",
                      "sql": "a destructive database schema change: {v}", "outside": "files role {role} may not change: {v}"},
           "nothing": "the project lead created no sub-ticket. Its report:\n\n{report}",
           "closed": "Every sub-ticket is closed:\n\n{list}", "states": {"done": "done", "cancelled": "cancelled"}},
    "fr": {"failed": "L'agent s'est arrêté avant la fin : {why}\n\nRépondez à ce commentaire pour qu'il réessaie.",
           "pr": "Pull request : {url}", "ticket": "Ticket {id}.",
           "refused": "Le travail est fait, mais les règles de l'agent interdisent d'ouvrir cette pull request : {why}.\n\n"
                      "Les commits sont sur la branche {branch} du fork. Répondez à ce commentaire (par exemple avec "
                      "une autre façon de faire) pour qu'il réessaie, ou ouvrez la pull request à la main.",
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
        "daily_tickets": int(env("AGENT_DAILY_TICKETS", "10")),
        "daily_usd": float(env("AGENT_DAILY_USD", "20")),
        "model": env("AGENT_MODEL"),
        "max_turns": env("AGENT_MAX_TURNS", "150"),
        "timeout": int(env("AGENT_TIMEOUT_MIN", "60")) * 60,
        "lang": env("AGENT_LANG", "en") if env("AGENT_LANG", "en") in TEXT else "en",
        "sweep": int(env("AGENT_SWEEP_MIN", "60")) * 60,
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


def summary(parent, words):
    """The closing report of a parent: its sub-tickets and how they ended."""
    return words["closed"].format(list="\n".join(
        f"- {k['id']} ({k.get('agent') or '-'}) : {k['title']}, {words['states'].get(k['state'], k['state'])}"
        for k in parent.get("children") or []))
def matches(path, pattern):
    """A pattern with a / against the whole path, one without against the file name."""
    return fnmatch.fnmatchcase(path if "/" in pattern else path.rsplit("/", 1)[-1], pattern)


def breaches(rules, numstat, diff):
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
    hits = [l[1:].strip()[:80] for l in diff.splitlines() if l.startswith("+") and not l.startswith("+++") and sql.search(l)]
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
    """Tickets taken and dollars spent today, kept in a file. Claude Code
    reports what a run would cost at API prices, subscription or not: the
    best measure of quota a headless run gives."""

    def __init__(self, path, tickets, usd):
        self.path, self.max_tickets, self.max_usd = path, tickets, usd

    def _read(self):
        today = datetime.date.today().isoformat()
        try:
            state = json.load(open(self.path, encoding="utf-8"))
        except (OSError, ValueError):
            state = {}
        return state if state.get("day") == today else {"day": today, "tickets": 0, "usd": 0.0, "paused": ""}

    def _write(self, state):
        json.dump(state, open(self.path, "w", encoding="utf-8"))

    def refusal(self):
        """Why no ticket is taken right now; None: go ahead."""
        s = self._read()
        if s["paused"]:
            return f"paused until tomorrow ({s['paused']})"
        if s["tickets"] >= self.max_tickets:
            return f"daily cap of {self.max_tickets} tickets reached"
        if s["usd"] >= self.max_usd:
            return f"daily budget of {self.max_usd:g} $ reached ({s['usd']:.2f} $ spent)"
        return None

    def state(self):
        """Today's count with the caps: what /status shows."""
        s = self._read()
        return {"tickets": s["tickets"], "max_tickets": self.max_tickets, "usd": round(s["usd"], 2), "max_usd": self.max_usd,
                "paused": s["paused"]}

    def add(self, tickets=0, usd=0.0, paused=""):
        s = self._read()
        s.update(tickets=s["tickets"] + tickets, usd=s["usd"] + usd, paused=paused or s["paused"])
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
    with urllib.request.urlopen(req, timeout=30) as resp:
        out = json.load(resp)
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
    base = {"HOME": pw.pw_dir, "PATH": f"{pw.pw_dir}/.local/bin:/usr/local/bin:/usr/bin:/bin", "LANG": "C.UTF-8",
            "GIT_AUTHOR_NAME": conf["git_name"], "GIT_AUTHOR_EMAIL": conf["git_email"],
            "GIT_COMMITTER_NAME": conf["git_name"], "GIT_COMMITTER_EMAIL": conf["git_email"]}
    # Output to files, not pipes: a process left in the background would hold
    # a pipe open and this program would wait for it instead of for the command.
    with tempfile.TemporaryFile("w+", encoding="utf-8", errors="replace") as out, \
            tempfile.TemporaryFile("w+", encoding="utf-8", errors="replace") as err:
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
    """Nothing of a ticket is left for the next: processes, clone, bundle."""
    try:
        as_worker(conf, ["sh", "-c", "kill -9 -1"], timeout=30)  # every process of the worker, this one included
        as_worker(conf, ["sh", "-c", 'rm -rf "$1"/job "$1"/out.bundle', "sh", os.path.join(conf["work"], "w")], timeout=120)
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
    cmd = ["claude", "-p", prompt, "--output-format", "json", "--max-turns", conf["max_turns"],
           "--mcp-config", mcp_file, "--strict-mcp-config", "--allowedTools", ",".join(role.get("allowed") or TOOLS),
           "--disallowedTools", ",".join(["WebFetch", "WebSearch", "Agent", "NotebookEdit", *role.get("denied", [])])]
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
        conf["budgets"][name] = Budget(os.path.join(conf["work"], file), conf["daily_tickets"], usd)
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
        if k["state"] == "todo" and not (role.get("waits") and waits_for(parent, k["id"])) and k["id"] not in queued:
            JOBS.put(job)
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
    # the role is read in the ticket, never in the call
    name = role_for(first, team) if team else "agent"
    role = team["roles"].get(name) if team else {}
    if role is None:
        return log(f"{tid}: role {name} is not in the team of {project}, left alone")
    say = lambda m: log(f"[{name}] {tid}: {m}")
    key = role.get("key")
    budget = budget_of(conf, name, float(role.get("usd", conf["daily_usd"])))
    why = budget.refusal()
    if why:
        return say(f"left alone, {why}")
    try:
        parent = mcp(conf, "get_ticket", key=key, id=first["parent"]) if first.get("parent") else None
        if role.get("waits") and parent and waits_for(parent, tid):
            return say(f"waits for {', '.join(waits_for(parent, tid))}")
        ticket = mcp(conf, "take_ticket", key=key, id=tid)
    except Refused as e:
        return say(f"not taken: {e}")
    say(f"taken ({call.get('woken_by') or call['event']})")
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
    auth = git_auth([repo["repo"], push], conf["git_user"], conf["git_token"])
    try:
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
        try:
            report, usd = claude(conf, job, fill(template, ticket, parent, ("Your role in the team: " + role["text"]) if role.get("text") else "", others), role)
        except RuntimeError as e:
            budget.add(usd=getattr(e, "usd", 0.0))
            if re.search(r"(?i)usage limit|rate limit|quota|credit balance|usage credits", str(e)):
                budget.add(paused="Claude said its limit is reached")  # this role only, its own budget
            raise
        budget.add(usd=usd)
        # nothing Claude left running may touch what comes next
        as_worker(conf, ["sh", "-c", "kill -9 -1"], timeout=30)
        # what Claude left uncommitted is part of the work
        as_worker(conf, ["git", "add", "-A"], cwd=job)
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
        if now["state"] != "in_progress" or asked(now, int(started)):  # Claude asked a question
            report_run(conf, key, run_id, state="waiting", cost_usd=usd)
            return say(f"now {now['state']}, question asked, {new} commit(s) pushed, {usd:.2f} $")
        report = report.encode()[:9000].decode(errors="ignore")  # a comment holds 10 000 bytes
        if ahead:
            span = f"origin/{base}...{branch}"
            numstat = git(["diff", "--numstat", "-z", "--no-renames", span], cwd=src)
            # the paths of the role: checked here, out of Claude's reach (the forge grants a whole repository)
            off = outside(role["paths"], numstat) if "paths" in role else []
            if off:
                raise Breach(text["breach"]["outside"].format(role=name, v=", ".join(off[:20])))
            found = breaches(conf["rules"], numstat,
                             git(["diff", "-U0", "--no-renames", "--no-color", "--no-ext-diff", span], cwd=src))
            stop = blocking(found, conf["rules"])
            if stop:
                raise Breach(say_breaches({"files": stop}, conf["rules"], text["breach"]))
            if found:  # the pull request is opened all the same; whoever merges reads rules.json again
                report = text["manual"].format(why=say_breaches(found, conf["rules"], text["breach"])) + "\n\n" + report
            body = f"{text['ticket'].format(id=tid)}\n\n{report}"
            url = open_pr(conf, repo, base, branch, f"{tid}: {ticket['title']}", body)
            mcp(conf, "add_link", key=key, id=tid, url=url, title=f"PR: {ticket['title']}"[:200])
            report += "\n\n" + text["pr"].format(url=url)
        cost = f"\n\n{name} · {role.get('model') or conf['model'] or '-'} · {usd:.2f} $"
        if role.get("splits"):
            # the lead splits and builds nothing: the parent stays in progress until its parts are closed
            parts = now.get("children") or []
            if not parts:
                raise RuntimeError(text["nothing"].format(report=report[:4000]))
            mcp(conf, "comment_ticket", key=key, id=tid, body=report + cost)
            report_run(conf, key, run_id, state="ok", cost_usd=usd)
            return say(f"split into {', '.join(k['id'] for k in parts)}, {usd:.2f} $")
        mcp(conf, "finish_ticket", key=key, id=tid, report=report + cost, minutes=max(1, round((time.time() - started) / 60)))
        report_run(conf, key, run_id, state="ok", cost_usd=usd)
        say(f"done, {ahead} commit(s), {usd:.2f} $")
        if team and parent:
            try:
                close_parent(conf, team, parent["id"], text)
            except Exception as e:
                say(f"parent {parent['id']} not updated: {e!r}")
    except Exception as e:  # whatever went wrong, the ticket must not stay "in progress" with nobody on it
        say(f"failed: {e!r}")
        report_run(conf, key, run_id, state="failed", cost_usd=usd or getattr(e, "usd", 0.0))
        why = str(e)[:1500]
        try:
            mcp(conf, "wait_ticket", key=key, id=tid, question=text["refused"].format(why=why, branch=branch)
                if isinstance(e, Breach) else text["failed"].format(why=why))
        except Exception as e2:
            say(f"could not even say so in the ticket: {e2!r}")
    finally:
        NOW["ticket"] = ""
        clean(conf)


def sweep(conf):
    """Queues what a lost call left behind (the container was down, the day's
    cap was reached): approved tickets nobody is assigned to, and waiting
    tickets whose last comment is from whoever approved them. A ticket
    assigned to someone is theirs and is left alone."""
    today = datetime.datetime.now(datetime.timezone.utc).date().isoformat()  # Cairn Plan wakes on the UTC day
    for project in sorted(conf["repos"]):
        try:
            for t in mcp(conf, "list_tickets", project=project)["tickets"]:
                # a ticket that sleeps until a later day is left: Cairn Plan calls when it wakes
                if not t.get("assignee") and not t.get("wake_date", "") > today:
                    JOBS.put({"event": "ticket.approved", "ticket": t["id"]})
            for t in mcp(conf, "list_tickets", project=project, state="waiting")["tickets"]:
                full = mcp(conf, "get_ticket", id=t["id"])
                last = (full.get("comments") or [{}])[-1].get("author")
                # a sub-ticket was approved by the lead, on the approval of its parent: either answers
                ap = full.get("approval") or {}
                if last and last in (ap.get("by"), ((ap.get("parent") or {}).get("approval") or {}).get("by")):
                    JOBS.put({"event": "ticket.answered", "ticket": t["id"]})
        except Exception as e:  # Cairn Plan not up yet, a project the account is not a member of
            log(f"sweep {project}: {e!r}")


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

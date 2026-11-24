#!/usr/bin/env python3
"""The page in a headless browser, against the binary just built.

Run by the Dockerfile (stage "screen"); by hand: python3 tests/screen.py
with cairn-plan and chromium on the PATH. It starts the server on a
throwaway database, a small proxy that adds the identity headers a real
proxy would set, makes a few tickets through the API, then loads each page
and checks what it shows. A page that fails to draw fails the build.
"""
import http.server, json, os, subprocess, sys, tempfile, threading, time, urllib.error, urllib.request

APP, PROXY = 19394, 19395
FENCE = "`" * 3
KEY = "k" * 40   # the key of the account "claude", which reports the runs of the Agents page
work = tempfile.mkdtemp()
with open(work + "/keys", "w") as f:
    f.write("test:" + KEY + "\n")
with open(work + "/config.json", "w") as f:
    json.dump({"listen": "127.0.0.1:%d" % APP, "trusted_proxies": ["127.0.0.1"], "admin_groups": ["admins"],
               "db": work + "/plan.db", "lang": "en", "mcp_keys_file": work + "/keys",
               # the team page: two members, the queue of worker is the tickets of bob
               "team": {"groups": [{"id": "core", "title": "Core team"}],
                        "members": [{"id": "worker", "name": "Worker", "group": "core", "look": "hardhat", "hue": "#e0803a",
                                     "agents": ["ouvrier"], "assignees": ["bob"]},
                                    {"id": "checker", "name": "Checker", "group": "core", "look": "glasses", "agents": ["controleur"]}]}}, f)
server = subprocess.Popen(["cairn-plan", work + "/config.json"])


def call(login, path, body=None):
    req = urllib.request.Request("http://127.0.0.1:%d/api/%s" % (APP, path), data=None if body is None else json.dumps(body).encode())
    req.add_header("Remote-User", login)
    req.add_header("Remote-Groups", "admins" if login == "alice" else "users")
    if body is not None:
        req.add_header("Content-Type", "application/json")
    return json.loads(urllib.request.urlopen(req).read())


def report_run(**run):
    """A run reported through the MCP tool, as an agent does."""
    body = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "report_run", "arguments": run}}
    req = urllib.request.Request("http://127.0.0.1:%d/mcp" % APP, data=json.dumps(body).encode(),
                                 headers={"Authorization": "Bearer " + KEY, "Content-Type": "application/json"})
    out = json.loads(urllib.request.urlopen(req).read())
    assert out["result"]["isError"] is False, out


class Proxy(http.server.BaseHTTPRequestHandler):
    """Everything the browser asks goes to the server as alice, an administrator."""
    def do_GET(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0)) or None
        req = urllib.request.Request("http://127.0.0.1:%d%s" % (APP, self.path), data=body, method=self.command)
        if self.headers.get("Content-Type"):
            req.add_header("Content-Type", self.headers["Content-Type"])
        req.add_header("Remote-User", "alice")
        req.add_header("Remote-Groups", "admins")
        try:
            r = urllib.request.urlopen(req)
        except urllib.error.HTTPError as e:
            r = e
        data = r.read()
        self.send_response(r.status if hasattr(r, "status") else r.code)
        for k in ("Content-Type", "Content-Security-Policy"):
            if r.headers.get(k):
                self.send_header(k, r.headers[k])
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    do_POST = do_GET

    def log_message(self, *a):
        pass


def dom(route):
    """The page at this route, once its scripts have run."""
    out = subprocess.run(
        ["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--disable-dev-shm-usage", "--user-data-dir=" + work + "/chromium",
         "--virtual-time-budget=15000", "--dump-dom", "http://127.0.0.1:%d/%s" % (PROXY, route)],
        capture_output=True, text=True, timeout=120)
    return out.stdout


try:
    for _ in range(50):
        try:
            urllib.request.urlopen("http://127.0.0.1:%d/healthz" % APP)
            break
        except OSError:
            time.sleep(0.2)
    threading.Thread(target=http.server.ThreadingHTTPServer(("127.0.0.1", PROXY), Proxy).serve_forever, daemon=True).start()

    # what the pages will show
    call("alice", "projects", {"key": "DEMO", "name": "Demo project", "space": "Lab"})
    call("alice", "projects/DEMO/members", {"login": "bob", "role": "contributor"})
    call("alice", "projects/DEMO/tickets", {"title": "Draw the plan", "description": "With a diagram.", "state": "todo"})
    call("bob", "tickets/DEMO-1", {"state": "in_progress", "assignee": "bob"})
    call("bob", "tickets/DEMO-1/comments", {"body": "The flow:\n\n" + FENCE + "mermaid\nflowchart LR\n  A[Ask] --> B[Build]\n" + FENCE})
    call("bob", "tickets/DEMO-1/work", {"minutes": 90, "day": time.strftime("%Y-%m-%d"), "note": ""})
    call("bob", "tickets/DEMO-1/comments", {"body": "Which colour for the roof?"})
    call("bob", "tickets/DEMO-1", {"state": "waiting"})
    call("bob", "projects/DEMO/tickets", {"title": "Give the key of the shed", "state": "waiting", "assignee": "alice", "description": "I need it."})
    call("bob", "projects/DEMO/tickets", {"title": "Paint the fence"})
    call("alice", "projects/DEMO/members", {"login": "claude", "role": "contributor"})
    now = int(time.time())
    report_run(run_id="s1", agent="ouvrier", source="claude-code", task="Sweep the shed", state="ok", started=now - 400, ended=now - 300,
               ticket="DEMO-2", model="claude-sonnet-5-5", cost_usd=0.12)
    report_run(run_id="s2", agent="controleur", source="claude-code", task="Check the shed", state="ok", started=now - 290, ended=now - 250,
               ticket="DEMO-2", result="conforme")
    report_run(run_id="s3", agent="ouvrier", source="claude-code", task="Paint the roof", state="running", started=now - 60)

    # route, what the page must show
    pages = [
        ("#/", ["Demo project"]),
        # before the tickets: opening a ticket reads its notices
        ("#/n", ["Notifications", "Mark all as read", "Draw the plan", "earlier notices"]),
        ("#/p/DEMO", ["Draw the plan", "Paint the fence", "Create and approve"]),
        ("#/t/DEMO-1", ["bob asks you", "Which colour for the roof?", "<svg", "Send the answer", "Edit the description", "Wake up"]),
        ("#/t/DEMO-2", ["bob asks you", "I need it.", "Done, close it"]),
        ("#/m", ["Waiting for your answer", "Give the key of the shed", "Draw the plan"]),
        ("#/p/DEMO/time", ["Time", "1"]),
        ("#/p/DEMO/members", ["Members of Demo project", "bob", "Export"]),
        ("#/a", ["Agents", "1 agent at work", "Paint the roof", "Sweep the shed", "conforme", "The team", "ouvrier", "controleur", "an hour over 30 days"]),
        ("#/u", ["My hourly rate"]),
        ("#/s", ["Settings", "Currencies", "Value of one dollar", "Price of a million tokens"]),
        ("#/q", ["Search", "Type two characters or more."]),
        ("#/agent", ["Agent", "No agent is set"]),
        ("team", ["The team", "Core team", "Worker", "Working", "Paint the roof", "Checker", "Resting"]),
        ("team#worker", ["What it did", "Sweep the shed", "In its queue", "DEMO-1"]),
    ]
    bad = []
    for route, wanted in pages:
        page = dom(route)
        missing = [w for w in wanted if w not in page]
        if 'class="banner"' in page:
            missing.append("(the page shows an error)")
        print("%-18s %s" % (route, "ok" if not missing else "MISSING " + ", ".join(missing)))
        if missing:
            bad.append(route)
    if bad:
        sys.exit("screen test failed: " + ", ".join(bad))
    with open("/screen.ok" if os.access("/", os.W_OK) else work + "/screen.ok", "w") as f:
        f.write("ok\n")
finally:
    server.terminate()

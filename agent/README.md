# Cairn Plan agent

A container that takes a ticket as soon as a project owner approves it,
lets [Claude Code](https://claude.com/claude-code) do the work in a clone
of the project's repository, and opens a pull request. It stops at the pull
request: it merges nothing and deploys nothing.

1. A project owner moves a ticket to "To do". Cairn Plan calls the agent
   (signed webhook, set on the Settings page).
2. The agent takes the ticket ("In progress", assigned to the MCP account),
   clones the repository, and runs Claude Code on the title and the
   description of the ticket.
3. Claude commits on a branch `agent/<ticket>`. The agent pushes it, opens
   a pull request titled `<ticket>: <title>`, links it to the ticket and
   closes the ticket with Claude's report and the time spent.
4. If Claude cannot go on without an answer, it asks in the ticket, which
   becomes "Waiting". When whoever approved the ticket answers in a comment,
   Cairn Plan calls the agent again and the work goes on from the branch.
5. If anything fails (the forge refuses the push, Claude stops, an hour
   passes), the agent says so in the ticket, which becomes "Waiting":
   answer the comment to have it try again.

One ticket at a time, in the order they were approved.

## What you need

- Cairn Plan with its MCP server on, and a key for the agent in
  `mcp_keys_file`. The MCP account (`claude` by default) must be a
  **contributor** of each project the agent works on, never an owner.
- A way for Claude Code to sign in: the token `claude setup-token` prints
  (Claude subscription), or an Anthropic API key.
- An account on your forge that can open pull requests, and an access token
  for it. Give it as little as that takes: on GitHub, a fine-grained token
  with "Contents" and "Pull requests" on the repositories concerned; on
  Forgejo or Gitea, an account that only reads the repository and pushes to
  its own fork (create the fork once).
- A secret of 16 characters or more shared with Cairn Plan, for the webhook.

## Run

```yaml
services:
  cairn-plan-agent:
    image: ghcr.io/jbellirigenty/cairn-plan-agent:0.14   # or build: ./agent
    restart: unless-stopped
    ports: ["127.0.0.1:9095:9095"]   # only Cairn Plan needs to reach it
    environment:
      PLAN_MCP_URL: https://plan.example.org/mcp
      PLAN_MCP_KEY_FILE: /run/agent/mcp-key
      AGENT_HOOK_SECRET_FILE: /run/agent/hook-secret
      AGENT_CLAUDE_TOKEN_FILE: /run/agent/claude-token
      AGENT_GIT_TOKEN_FILE: /run/agent/git-token
      AGENT_GIT_USER: plan-bot
      AGENT_REPOS: >-
        {"SHOP": {"repo": "https://git.example.org/team/shop.git",
                  "push": "https://git.example.org/plan-bot/shop.git"}}
    volumes:
      - ./agent-secrets:/run/agent:ro   # files owned by root, mode 0400
      - ./agent-work:/work
    cap_drop: [ALL]
    cap_add: [SETUID, SETGID, CHOWN]    # to run Claude Code as another user
    security_opt: ["no-new-privileges:true"]
```

Then, on the Settings page of Cairn Plan, section "Agent": the address
`http://127.0.0.1:9095/hook` (or wherever the container listens), the same
secret, and "Send a test": the agent answers.

| Variable | Default | |
|---|---|---|
| `PLAN_MCP_URL` | required | Address of Cairn Plan's MCP server. |
| `PLAN_MCP_KEY_FILE` | required | File holding a key of `mcp_keys_file`. |
| `AGENT_HOOK_SECRET_FILE` | required | File holding the secret of the webhook, 16 characters or more. |
| `AGENT_CLAUDE_TOKEN_FILE` | one of the two | File holding the token of `claude setup-token`. |
| `AGENT_ANTHROPIC_KEY_FILE` | one of the two | File holding an Anthropic API key. |
| `AGENT_GIT_TOKEN_FILE` | required | File holding the access token of the forge account. |
| `AGENT_GIT_USER` | `x-access-token` | User name sent with the token (GitHub accepts the default; Forgejo wants the account name). |
| `AGENT_GIT_NAME`, `AGENT_GIT_EMAIL` | `Cairn Plan agent`, `agent@localhost` | Author of the commits. |
| `AGENT_REPOS` | required | JSON, one entry per project key: `repo` (https clone address), `push` (where the branch goes; a fork; default: `repo`), `base` (branch the pull request targets; default: the default branch). A ticket of a project that is not listed is left alone. |
| `AGENT_DAILY_TICKETS` | `10` | Tickets taken per day. Beyond, tickets stay "To do". |
| `AGENT_DAILY_USD` | `20` | Daily budget, in dollars at API prices as Claude Code reports them for each run (on a subscription nothing is billed: it is the measure of quota a run gives). Beyond, tickets stay "To do". |
| `AGENT_MODEL` | Claude Code's | Model, e.g. `sonnet`. |
| `AGENT_MAX_TURNS` | `150` | Turns of one run. |
| `AGENT_TIMEOUT_MIN` | `60` | Minutes of one run. |
| `AGENT_LANG` | `en` | Language of the agent's own messages in a ticket, `en` or `fr`. Claude's report follows the language of the ticket. |
| `AGENT_SWEEP_MIN` | `60` | Minutes without a call after which the agent looks for tickets a lost call left behind. `0`: only when it starts. |
| `AGENT_LISTEN` | `0.0.0.0:9095` | Address and port inside the container. |

`/healthz` answers `ok`. The log says what happened to each call:
`SHOP-52: taken`, `SHOP-52: done, 2 commit(s), 0.84 $`, or why a ticket
was left alone.

## What the agent trusts

- **The webhook** only says which ticket to look at; its signature is
  checked (HMAC-SHA256, header `X-Hub-Signature-256`). The agent then reads
  the ticket itself, and Cairn Plan refuses to hand over a ticket that no
  owner approved.
- **The title and the description** of the approved ticket are the request.
  Comments, links, file names and whatever Claude reads while working are
  data: the prompt ([prompt.md](prompt.md)) says so, and Cairn Plan lets
  only an owner change the text of an approved ticket.
- **Claude Code runs as an unprivileged user** that sees neither the forge
  token nor the webhook secret. Its commits come back as a git bundle, so
  no hook or configuration it wrote runs with the token. It cannot take or
  close a ticket by itself: the agent does both.
- **Rules** in [rules.json](rules.json) (`AGENT_RULES_FILE` to use another
  file), checked by this program on the commits, not by Claude. Files in
  `blocking` (CI workflows, `.env`, secrets): the change is pushed to its
  branch but no pull request is opened, and the ticket waits with the
  reason. The rest (Dockerfiles, compose and deployment files, migrations,
  destructive schema changes, binary files, more than 400 changed lines or
  20 files): the pull request is opened and says it is for a person to
  merge. Additive schema changes and new dependencies are fine. Whatever
  merges the pull requests automatically should read the same file again
  and leave a breach to a person.

## Limits

- Claude can run any command inside the container, with the network the
  container has. It can read its own Claude token and the MCP key. Keep the
  container on a network that reaches the forge, Cairn Plan and Anthropic,
  and little else, and approve tickets with that in mind.
- It builds and tests with what the image holds: git, Python, a shell. A
  project that needs a toolchain to be checked relies on the CI of the pull
  request, or on an image built `FROM` this one with the toolchain added.
- A call that arrives while the container is down is lost, and so is one
  refused by the daily cap or budget. The agent catches up when it starts
  and whenever nothing came for `AGENT_SWEEP_MIN` minutes: it takes the
  approved tickets of its projects that nobody is assigned to, and the
  waiting ones that whoever approved them answered. A ticket assigned to
  someone is left to them. The counters of the day are kept in
  `/work/state.json`.
- A container stopped in the middle of a ticket leaves it "In progress":
  put it back to "To do" by hand.
- When Claude says its usage limit is reached, the agent takes nothing more
  until the next day.
- A pull request opened from a fork may wait for an approval before the CI
  of the forge runs: that is the forge's rule, and a good one.

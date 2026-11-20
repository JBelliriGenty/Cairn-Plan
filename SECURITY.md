# Security

Please report a vulnerability privately, through GitHub's "Report a
vulnerability" button on the Security tab of this repository, not in a
public issue. You will get an answer within a week.

Only the latest release is maintained.

What Cairn Plan relies on, and what to check first if you deploy it:

- **The reverse proxy is the sign-in.** Cairn Plan believes the
  `Remote-User` / `Remote-Groups` headers only from the addresses in
  `trusted_proxies`. The proxy must overwrite those headers, and nothing
  but the proxy must reach the port (bind `listen` to a private address and
  firewall it).
- **Keys and secrets live in files** (`mcp_keys_file`,
  `webhook_secrets_file`, `smtp_credentials_file`, `ntfy_token_file`,
  `agent_hook_secret_file`), readable by the process only. Keys of 32
  characters or more for MCP; 16 or more for webhook secrets.
- **The agent runs Claude Code** with the network and the forge token you
  give it. Give the forge account only what opening a pull request takes,
  and let a person merge what `agent/rules.json` flags. See
  [agent/README.md](agent/README.md#limits).

# typeduck-codex-web

`typeduck-codex-web` is a standalone Codex runtime with a browser interface. It
includes the app server, so it does not require another `codex` executable.

## Install

```console
cargo install typeduck-codex-web
```

Start the user daemon and open its project dashboard:

```console
typeduck-codex-web
```

The dashboard discovers recent projects and sessions from your Codex history. Use
the directory picker to start work anywhere the daemon account can access, or open
a specific directory directly:

```console
typeduck-codex-web -C /path/to/project
```

Recent sessions are grouped by activity time. Inside a conversation, use **Goal**
to manage its persisted objective, lifecycle status, token budget, and usage. The
Settings dialog is available from both the dashboard and conversation header for
theme selection, complete MCP configuration and authentication, and ordered
account management.

On startup the command prints a private bootstrap URL. Opening it establishes an
HTTP-only browser session scoped to an unguessable server path; the persistent
secret remains in `CODEX_HOME`. Use `--reset-token` to revoke existing browser
sessions.

Run `typeduck-codex-web --help` for port, browser, working-directory, and
configuration options.

## Accounts and proxies

Create `$CODEX_HOME/typeduck-codex-web/daemon.toml` to configure accounts in
fallback order. The first profile is the normal Codex CLI account and continues
to use the shared `$CODEX_HOME` for config, MCP servers, goals, and sessions.
Additional profiles store only private credentials under the web daemon's data
directory. At startup and before new turns, the daemon selects the first
signed-in account whose reported rate limit is still available. It never
automatically replays a failed turn.

```toml
port = 36915
mcp_oauth_callback_url = "http://127.0.0.1:36915/oauth/callback"

[[accounts]]
name = "primary"
label = "Work"
role = "primary"
proxy = "http://127.0.0.1:8080"

[[accounts]]
name = "backup"
label = "Personal"
role = "fallback"
use_ssh_tunnel = true

[ssh_tunnel]
destination = "me@bastion.example.com"
identity_file = "/home/me/.ssh/id_ed25519"
ssh_port = 22
# local_port = 1080 # omit to choose an available loopback port
```

Proxy URLs may use `http`, `https`, `socks5`, or `socks5h`. The daemon applies
the selected account's proxy to Codex-owned OpenAI HTTP and WebSocket clients.
For `use_ssh_tunnel`, it owns an OpenSSH `ssh -N -D` child process and stops it
with the daemon. The SSH feature therefore requires `ssh` on `PATH`; the Codex
runtime itself remains embedded in the installed binary.

Use `--daemon-config /path/to/daemon.toml` to load another file.
The optional `port` and `mcp_oauth_callback_url` values keep MCP OAuth redirect
URLs stable across daemon restarts. CLI `--port` takes precedence when nonzero.
Profile additions, edits, removals, sign-in, switching, and fallback-order
changes made in Settings apply without restarting. Account switching waits for
the current turn to become idle.

## Restart recovery

The daemon records top-level sessions while they are running. When
`typeduck-codex-web` starts again, it reopens sessions that were interrupted by
the previous shutdown and continues them with their last account, model,
reasoning effort, and collaboration mode. Active goals use Codex's native goal
continuation; other interrupted turns begin with a recovery instruction that
checks persisted workspace progress before repeating any action.

Approvals still wait for user input. Live model streams and child processes
cannot survive a machine reboot, so recovery starts a new turn from the durable
thread history rather than restoring the exact operating-system process. Starting
the binary after boot remains the responsibility of the host's service manager
or another supervisor.

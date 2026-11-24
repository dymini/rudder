# Rudder threat model

Repository: `vantanminh/rudder`. Rudder is a self-hosted deployment platform: one panel (control plane + embedded web UI) per VPS, optional remote servers running an agent, Traefik for routing/TLS, an optional Pingora-based HTTP cache, and a `rudder` CLI. Infrastructure is in-repo (`deploy/install.sh`, `deploy/install-agent.sh`, `deploy/docker/*.Dockerfile`); there is no separate infrastructure repository.

## Components

| Component | Code | Notes |
| --- | --- | --- |
| **Panel API** | `crates/server/src/routes/`, `auth.rs`, `audit.rs`, `deploy.rs`, `stacks.rs`, `backup/`, `security.rs`, `update.rs`, `crates/core/` | HTTP API on :3000 (or HTTPS via Traefik). Cookie sessions (SHA-256 hashed, CSRF token, SameSite=Lax), scoped API tokens, TOTP, admin/viewer roles, owner account. |
| **MCP & OAuth** | `crates/server/src/mcp/`, `routes/mcp.rs` | `/mcp`, dynamic client registration `/oauth/register`, PKCE S256, scoped grants (`rudder_core::MCP_SCOPES`). |
| **Rudder Access & edge gate** | `routes/access.rs`, `gate.rs`, `routes/edge.rs`, `crates/core/src/access.rs`, `ratelimit.rs` | Portal `/access` (Google OAuth, email code, Turnstile), per-host cookies via `/_rudder/callback`, identity headers to apps, introspect API, Traefik forward-auth gate with a secret path. |
| **Agent link** | `crates/server/src/agent/`, `driver/remote.rs`, `crates/agent/`, `crates/proto/` | Join-token enrollment, mTLS gRPC on :7443, agent executes Docker operations and self-updates. |
| **Build & runtime driver** | `crates/node/`, `build_queue.rs`, `crates/core/src/{buildspec,compose,traefik,template,isolate,envfile}.rs`, `templates/` | Git/Dockerfile/Nixpacks builds, Compose stacks, Traefik config generation, Docker socket access (panel container mounts `/var/run/docker.sock`). |
| **Webhooks & GitHub App** | `routes/git.rs`, `routes/github_app.rs`, `routes/github_repos.rs`, `crates/core/src/{webhook,github_app}.rs`, `gitkey.rs` | Public push webhooks verified with `X-Hub-Signature-256` HMAC; GitHub App private key, deploy keys. |
| **Web UI** | `web/` | SPA embedded in the server; logs live tail and container terminal over WebSocket (`routes/logs.rs`). |
| **Cache** | `crates/cache/` | Per-server reverse proxy cache; admin listener behind a bearer token. |
| **CLI** | `crates/cli/` | Login (API key, browser, device code), local builds, R2 push, self-update, decrypt. |

## Trust boundaries and entry points

- Internet → Panel API / Web UI (login, setup, all `/api/v1/*`). `/api/v1/setup` is open only while no user exists.
- Internet → MCP/OAuth endpoints (registration and token endpoints are unauthenticated; approval requires a signed-in admin browser).
- Internet → Rudder Access portal and every gated app domain via Traefik → gate.
- Internet → GitHub webhooks (`routes/git.rs`, `routes/github_app.rs`), HMAC-authenticated.
- Internet → deployed apps and the cache proxy (ports 80/443). Apps should not reach the panel's internals; isolation keeps apps off host ports.
- Remote server ↔ panel: agent dials out on :7443 with mTLS after one-time join token; panel sends Docker commands.
- Panel/agent → Docker daemon (root-equivalent on the host).
- Panel → external services: S3/R2 (backups), Cloudflare API, Resend/SMTP, Telegram/Discord/Slack/webhook alert targets, GitHub API, GitHub Releases (self-update, signed).
- Viewer vs admin vs owner, and token/MCP scope boundaries within the panel.

## Assets (user: all equally important)

- Root/host control on every server (Docker socket, agent RCE, container escape, self-update channel).
- Stored secrets: app env vars, database credentials, S3/R2, Cloudflare, GitHub App key, email provider keys, encrypted backups and their keys, API/MCP tokens, session cookies.
- Internal apps protected by Rudder Access, and the identity headers they trust.
- Panel integrity: users, roles, audit log, routing (Traefik config), TLS certificates.

## Attacker profiles

- **Unauthenticated internet attacker** (user-confirmed: matters). Reaches panel login/setup, OAuth/MCP endpoints, webhooks, Access portal, gate, apps, cache.
- **Panel accounts are created only by an admin** (user answer): there is no public sign-up; panel users are trusted operators/teammates. A **viewer** or narrowly scoped token/MCP grant trying to exceed its role or scope still matters (user: every attacker matters).
- **Rudder Access users**: allow-listed coworkers; one may try to reach apps they are not granted or impersonate others.
- **Compromised deployed app / container**: tries to reach the panel, Docker, other apps, or forge Access headers.
- **Compromised or malicious remote server/agent**: tries to attack the panel over gRPC or other servers.
- **Malicious AI agent** connected via MCP with a limited grant.
- **Network attacker** on HTTP-only panels or between panel and agent.

## Existing controls

Hashed session cookies + CSRF; TOTP; owner protection; scoped tokens and MCP scopes with admin-only approval; PKCE S256; HMAC webhook verification; join tokens hashed with expiry, mTLS for agents; Turnstile + verified email + allow list for Access; per-host HttpOnly Access cookies; gate secret path; secrets encrypted at rest and not returned by the API; log masking (`crates/core/src/redact.rs`); signed self-updates; constant-time cache admin token check; rate limits.

## Out of scope / accepted risk

User answer: nothing is ruled out — every attacker matters. No accepted risks recorded.

## Open questions

- Is network isolation on by default for every deployment (headers are only trustworthy with it)?
- Are panels commonly exposed over plain HTTP on :3000?

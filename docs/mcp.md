# AI agents

An AI agent connects to Dymini Panel over MCP (the Model Context Protocol) and calls the same actions as the panel. You decide what that agent is allowed to do when you approve it.

## Connect an agent

1. In the agent, add a remote MCP server. The address is the panel's public address with `/mcp` on the end, for example `https://panel.example.com/mcp`.
2. The agent opens a browser page on the panel. Sign in if you are not already.
3. Choose a permission template, or pick permissions one by one, then approve.
4. The agent receives a short-lived access token and a refresh token. It refreshes on its own until you revoke it or the approval reaches 90 days.

The address Dymini Panel gives agents is the panel domain saved under **Certificates → Panel domain**. Without one it falls back to `DYMINI_PANEL_PUBLIC_URL`, then to the address the request came in on. An agent that is not on the same machine (Claude, for example) needs an HTTPS domain: save one and press **Check SSL** on the same page to confirm the certificate is trusted, covers the domain, and is not about to expire.

**Admin → AI agents** lists every approval, the template, and the last time it was used. **Revoke** cuts off the access token and the refresh token immediately.

## OAuth client compatibility

OAuth authorization accepts non-empty `state` values up to 4096 bytes and echoes them unchanged in the callback after approval. This supports agents that encode longer sign-in context in state, such as Devin.

Dynamic registration at `/oauth/register` supports public clients (`none`, the default) and confidential clients (`client_secret_post` or `client_secret_basic`), including server-side agents such as Devin. `client_name` is optional and defaults to “MCP client”; extra registration fields are ignored. Confidential registrations return a client secret once, with no expiry; Dymini Panel stores only its hash. Keep that secret private. For token exchanges, refreshes, and revocation, confidential clients may send the secret through either HTTP Basic or the request body, regardless of which secret method they registered. Public clients must not send a secret. Basic credentials are form-URL-encoded before being joined with `:` and base64-encoded. All authorization code exchanges still require S256 PKCE.

Discovery is available at both `/.well-known/oauth-protected-resource` and `/.well-known/oauth-protected-resource/mcp`, and at both `/.well-known/oauth-authorization-server` and `/.well-known/oauth-authorization-server/mcp`.

Registration reads JSON regardless of the Content-Type header. Optional metadata may be omitted or null; unknown fields are ignored. Requested grant and response types are replaced with `authorization_code` plus `refresh_token`, and `code`. Unsupported authentication methods (such as `private_key_jwt`) fall back to `client_secret_post`, with a generated secret; clients should use the metadata returned by registration. Redirect URLs are still validated. Rejections return `error` and `error_description` in the standard OAuth registration format and are logged with the reason, client name, and redirect URLs, without client secrets.

## API keys created by an agent

The `post_v1_tokens`, `get_v1_tokens`, and token-delete tools use `auth:write`
and `auth:read`. Creation returns the secret once; lists never return secrets.
The key records its creating MCP client and grant. Its permissions are bounded
by that grant and checked again on every request: reducing or revoking the grant
also reduces or revokes its delegated keys. Ordinary browser-created keys keep
their existing behavior.

The optional `agent_scopes` and `allowed_tools` fields request narrower access.
When either is supplied, omitted permission fields mean no permissions. Without
either, the key inherits the grant's permissions, limited by the requested
`read`/`write` API-key scopes. Requests exceeding the grant are rejected with
`permission_escalation`, never silently expanded. Scoped keys cannot bypass MCP
permissions by calling the HTTP API directly or by creating another key.

## Permission templates

| Template | What the agent can do |
| --- | --- |
| **Full control** | Read and change every setting and all data, including users, tokens, and updates. |
| **Read everything** | Read every setting and all data. Cannot change anything. |
| **Operate** | Deploy and manage apps, domains, databases, stacks, caches, logs, monitors, Dymini Panel Access, and security checks. Can read the Cloudflare Zero Trust settings but not change them. Cannot change users, tokens, backups, or panel updates. |
| **Watch** | Read health, logs, metrics, cache stats, and security checks. |
| **Custom** | Only the boxes you tick. A write permission includes the matching read permission. |

Only an administrator can approve an agent, and only from a signed-in browser. An agent cannot approve a wider grant for itself.

### Groups and individual tools

Choose **Custom** on consent to expand groups and select read/change scopes or
individual tools. A tool is allowed when either its scope is granted or its name
is explicitly selected. Group scopes include future tools in that scope;
individual selections do not. To remove one tool from a group, clear the group
scope and select the tools you want instead. The agent's `tools/list` advertises
only its permitted tools. Self-inspection (`dymini_panel_whoami`) is always available.

Use **AI agents → Edit permissions** to change an active approval without signing
in again. The same checks protect direct HTTP requests and delegated API keys.
Edits made through an agent cannot exceed that agent's own permissions. The
catalog includes tool names, descriptions and scopes; `PUT /api/v1/mcp/grants/{id}`
accepts `template`, `scopes`, and `allowed_tools` under `auth:write`.

## Reading container logs

With `logs:read`, `get_v1_services__id__logs` returns recent container logs of any service (`tail`, default 200). For a Compose stack it combines every compose service under `==> name <==` headers. To read one compose service, call `get_v1_services__id__stack_logs` (`stacks:read`) with `service=<name>`; omit `service` to get the combined logs, and read the stack's service names from `services`. Secret environment values are masked as `***`: values under a credential-looking key (`*_PASSWORD`, `*_TOKEN`, `*_SECRET`, `*_KEY`, `DATABASE_URL`, …), values that look generated, and passwords inside connection URLs. Masking applies to whole tokens only, and booleans, `null` and short numbers are never masked. Live tail and the container terminal are browser-only (WebSocket) and are not MCP tools.

## AI activity and retention

Open **AI agents → AI activity** for recent tool calls, success/error/denied rates,
UTC hourly or daily charts, top tools, counts by client, p50/p95 latency, and last
tool use per grant. Filters apply to both history and statistics; history is
paginated. Choose a 1-, 7-, or 30-day window. Empty time buckets contain zeroes.

Every tool call records its grant/client identifiers, canonical tool name, scope,
result (`ok`, `denied`, or `error`), target HTTP status, duration and timestamp.
Arguments, response bodies, credentials and secrets are never recorded. Error
summaries are deliberately generic; read the tool response for the detailed
reason. Unknown tool names are stored as `unknown`, not as caller-supplied text.
Unrecognized credentials have no client/grant attribution; recognized expired
or revoked access tokens retain their attribution while their hash exists.
`dymini_panel_whoami` has the synthetic scope `self`.

A durable entry is created before dispatch. If it cannot be written, the action
does not run. Completed calls are finalized afterwards. If finalization fails,
the action's response is still returned (especially one-time secrets), and the
entry remains an incomplete-execution error. In-flight calls are excluded from
history/statistics until finished; the hourly maintenance task finalizes entries
unfinished for at least 15 minutes and prunes calls older than 30 days. Queries
never include data outside their requested window, even before pruning runs.

Both APIs require `monitoring:read` for agents and an administrator account:

- `GET /api/v1/mcp/calls`: `client_id`, `tool`, `status`, `days` (1–30, default 7),
  `limit` (1–500, default 100), `offset` (0–100000).
- `GET /api/v1/mcp/stats`: the same client/tool/status/days filters, and `bucket`
  (`hour` or `day`, default `hour`). Rates are fractions from 0 to 1. Latency
  percentiles include completed allowed, denied and failed calls, in milliseconds.

The APIs are also MCP tools. Calling them through MCP contributes a call after
the response has been collected. `http_status` describes the target API result
(400 for invalid tool arguments, 500 for dispatch errors), not the JSON-RPC
transport's usual HTTP 200 response.

## How refresh works

- The access token lasts 15 minutes.
- The refresh token lasts 30 days and is replaced on every refresh. The previous refresh token stops working.
- Presenting a refresh token that was already replaced revokes that approval. The agent must be approved again.
- An approval that is older than 90 days cannot be refreshed. Approve the agent again.

## When you add a feature

Every panel feature is an MCP tool. A new HTTP route becomes a tool automatically, under the permission scope of its OpenAPI tag. Full control includes that tool. See the MCP parity rule in `AGENTS.md`.

## Search the documentation

Every panel carries its own copy of these guides, so an agent reads the documentation of the exact version you run, from the address it is already connected to. Three tools cover it (they need the `system:read` permission, which every template includes):

| Tool | HTTP | Use |
| --- | --- | --- |
| `get_v1_docs_search` | `GET /api/v1/docs/search?q=…&limit=5` | Ranked matches: guide, section, and a snippet. `limit` is at most 20 |
| `get_v1_docs__name_` | `GET /api/v1/docs/{name}` | One whole guide as Markdown, for example `cli` |
| `get_v1_docs` | `GET /api/v1/docs` | The guide names and titles |

Pass `q` as a query argument: `{"query": {"q": "deploy an image"}}`. A custom approval needs the **System** read permission ticked to use them.

## Teach agents about Dymini Panel (skills)

`dymini-panel skills install` puts a Dymini Panel skill where AI coding agents look for skills. It explains what Dymini Panel is, how to use the CLI and MCP tools, which actions need your confirmation, and how to search these docs. See [Skills for AI agents](cli.md#7-skills-for-ai-agents) in the CLI guide.

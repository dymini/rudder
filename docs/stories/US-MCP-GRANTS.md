---
contract: null
created_at: "2026-11-22T17:03:51.956301727+00:00"
e2e: 0
evidence: "Server library 89, auth 7, MCP 7, delegation/grants/usage 5 and OpenAPI 2 tests passed. Offline workspace all-target clippy and all-target SQLx prepare passed. Web i18n (1735 used, 1865 translated), TypeScript/Vite build and 11 existing web tests passed."
id: US-MCP-GRANTS
integration: 1
lane: high_risk
notes: "IN-039 remains unique. Implemented auth-scoped bounded one-time API keys, union-of-scope/tool grants with live edits, and private retained MCP usage under monitoring:read. Usage errors are intentionally generic, unknown tool names are not persisted, and dispatch fails closed if its initial call record cannot be saved. Browser appearance and live external OAuth clients are unverified. Merged origin/main 877b28c via 6c4b39d without content changes or conflicts."
platform: 0
status: implemented
title: "Delegate API keys safely, choose MCP tools, and monitor AI activity"
type: story
unit: 1
updated_at: "2026-11-22T17:24:50.126495889+00:00"
verify: "cargo fmt --all --check; SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings; DATABASE_URL=postgres://rudder:rudder@localhost:5432/rudder cargo sqlx prepare --workspace -- --all-targets; DATABASE_URL=postgres://rudder:rudder@localhost:5432/rudder SQLX_OFFLINE=true cargo test -p rudder-server --lib --test mcp --test mcp_grants --test auth --test openapi_snapshot --no-fail-fast; pnpm --dir web check:i18n; pnpm --dir web build; node --test web/scripts/ansi.test.mjs web/scripts/validation.test.mjs"
---

# Delegate API keys safely, choose MCP tools, and monitor AI activity

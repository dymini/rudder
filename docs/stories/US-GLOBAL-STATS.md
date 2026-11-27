---
contract: null
created_at: "2026-11-22T17:26:18.911222946+00:00"
e2e: 0
evidence: "Server library 89, monitoring 3, statistics 3, MCP 7, grants 5 and OpenAPI 2 tests passed. SQLx prepare --all-targets, formatting and offline workspace all-target clippy passed. Web i18n 1773 used/1902 translated, TypeScript/Vite build and 11 existing web tests passed."
id: US-GLOBAL-STATS
integration: 1
lane: high_risk
notes: "IN-040. Added indexed, bounded rollup/raw aggregation with a repeatable-read snapshot, proxy rate-to-count conversion, equal-server resource averages and rankings across service moves. Network totals and restart increases are sampled estimates; partial edge buckets are approximate. Backup partial outcomes count as failures; restores are excluded, and last successful backup is lifetime. Domain counts are service-domain rows; disk snapshots look back up to 30d. Browser/mobile rendering, real production traffic and production-scale query latency remain unverified. Initial seeded-test domain fields were incomplete; fixed after reading the full schema, and final aggregation tests pass."
platform: 0
status: implemented
title: View range-based global Dymini Panel statistics
type: story
unit: 1
updated_at: "2026-11-22T17:41:56.474717167+00:00"
verify: "cargo fmt --all --check; SQLX_OFFLINE=true cargo clippy --workspace --all-targets -- -D warnings; cargo sqlx prepare --workspace -- --all-targets; cargo test -p dymini-panel-server --lib --test stats --test monitoring --test mcp --test mcp_grants --test openapi_snapshot --no-fail-fast; pnpm --dir web check:i18n; pnpm --dir web build; node --test web/scripts/ansi.test.mjs web/scripts/validation.test.mjs"
---

# View range-based global Dymini Panel statistics

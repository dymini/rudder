# Contributing to Dymini Panel

Thanks for your interest in contributing! This document explains how to set up the project and what a pull request needs to be accepted.

## Ways to contribute

- **Report bugs** — open a [bug report](https://github.com/dymini/panel/issues/new?template=bug_report.yml). Include the panel version, how you installed it, and what you expected versus what happened.
- **Suggest features** — open a [feature request](https://github.com/dymini/panel/issues/new?template=feature_request.yml) or start a [discussion](https://github.com/dymini/panel/discussions) first for anything large.
- **Improve docs** — everything under `docs/` and the guides there are fair game; small fixes can go straight to a PR.
- **Write code** — read the rest of this document first.

For security issues, **do not open public issues** — see [SECURITY.md](SECURITY.md).

## Repository layout

| Path | What it is |
| --- | --- |
| `crates/` | Rust workspace: `server` (the panel), `cli` (`dymini-panel`), `agent`, `core`, `proto` |
| `web/` | The panel web UI (React + TypeScript + Vite, pnpm) |
| `app/` | The mobile app (Expo/React Native, bun) |
| `deploy/` | Installer scripts, Dockerfiles, release packaging |
| `templates/` | One-click app templates shipped with the panel |
| `docs/` | Operator and developer documentation |
| `docs/stories`, `docs/decisions`, `docs/intakes` | Durable project history managed by the [5harness](docs/HARNESS.md) CLI — **do not hand-edit** |

## Development setup

### Rust workspace

- Rust stable (see `rust-toolchain.toml`) with `cargo` and `clippy`.
- PostgreSQL 16 client tools (`psql`) and a scratch database for tests; migrations are managed with [sqlx](https://github.com/launchbadge/sqlx) and compile-time checked offline against `.sqlx/`. After changing a `sqlx::query!` macro, regenerate with `cargo sqlx prepare --workspace` against a database that has all migrations applied.
- Verify with: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

### Web UI

- Node + pnpm. In `web/`: `pnpm install`, then `pnpm build` (includes typecheck), `pnpm test` for the node tests.
- The web API types in `web/src/api-types.ts` are generated from `crates/server/openapi.json` — regenerate both after changing routes.

### Mobile app

- bun. In `app/`: `bun install`, `bunx tsc --noEmit`, `bunx expo lint`.

## What a pull request needs

1. **A changelog line.** `CHANGELOG.md` is what operators read in the panel before pressing *Update*. Add one line under `## [Unreleased]` in the right section, written for the person running the panel — what they can now do or what changed for them. Pure internal work (tests, CI, invisible refactors) needs no line. CI fails PRs that touch `crates/`, `web/src/`, `deploy/` or `templates/` without touching `CHANGELOG.md`.
2. **Docs in sync.** If behavior changes, update the matching guide under `docs/`.
3. **MCP parity.** Every user-facing feature must be controllable through the panel's MCP server: expose it on the HTTP API, register the route in `crates/server/src/openapi.rs`, and use an OpenAPI tag that has an MCP scope. See `AGENTS.md` for the exact rules.
4. **Tests.** Rust changes should come with unit or integration tests under `crates/server/tests/`. Web changes should keep `pnpm test` green.
5. **Green CI.** Formatting, clippy, tests, web build, and the changelog check all run in CI.
6. **Small and focused.** One thing per PR. Large features are easier to land as a series of small PRs — open a discussion first so we can agree on the direction.

## Commit style

Conventional commits: `feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `test:`, `ci:`. Breaking changes get a `!` (`feat!: …`) and a migration note in the changelog.

## Project management

This repository uses [5harness](docs/HARNESS.md) for durable work tracking (stories, decisions, intakes as git-backed markdown). You do not need harness tooling to contribute code; maintainers will record the durable entries. If you are an AI agent working in this repo, read `AGENTS.md` first.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).

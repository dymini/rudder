# Contributing to Dymini Panel

Thanks for your interest in contributing! This document explains how to set up the project and what a pull request needs to be accepted.

## Ways to contribute

- **Report bugs** — open a [bug report](https://github.com/dymini/panel/issues/new?template=bug_report.yml). Include the panel version, how you installed it, and what you expected versus what happened.
- **Suggest features** — open a [feature request](https://github.com/dymini/panel/issues/new?template=feature_request.yml) or start a [discussion](https://github.com/dymini/panel/discussions) first for anything large.
- **Improve docs** — everything under `docs/` and the guides there are fair game; small fixes can go straight to a PR. The guides listed in `developers.yml` are published to [developers.dymini.com/panel](https://developers.dymini.com/panel) for every release (see [Docs site](#docs-site)).
- **Translate docs** — put a translation at `docs/i18n/<language>/<same file name>` (for example `docs/i18n/vi/cli.md`); see [Docs site](#docs-site).
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
| `docs/i18n/<language>/` | Translations of the guides for developers.dymini.com |
| `developers.yml` | Which guides developers.dymini.com publishes, in which order and languages |
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

## Docs site

The guides are published to [developers.dymini.com/panel](https://developers.dymini.com/panel): every release tag
publishes its own version (older versions stay readable), and the `main` branch is not published until it is
released. Only pages listed in `developers.yml` appear there; design notes and project history stay in the repository.

- **Check locally:** `npx --yes https://cdn.dymini.com/lib/dymini-docs@1.0.1/dymini-docs.tgz dev` serves the site at
  http://localhost:4321 and rebuilds on save; `… build --strict` is what CI runs (broken links, missing pages,
  unsupported MDX).
- **Pull requests** from branches of this repository get a preview at
  `https://developers.dymini.com/_preview/panel/pr-<number>` for 14 days.
- **Translations:** copy a guide to `docs/i18n/<language>/` and translate it, keeping links and file paths as in the
  original. Put the hash of the original you translated from in its frontmatter (`translatedFrom: …`);
  `… i18n status --locale vi` prints it and lists missing or out-of-date translations. A page whose original changed
  after translation shows a note until `translatedFrom` is updated. Languages are listed under `locales` in
  `developers.yml`.
- **New guide:** add it to `nav` in `developers.yml`. The built-in guides served by the panel itself
  (`/api/v1/docs`) are the top-level `docs/*.md` files and do not include translations.

## Commit style

Conventional commits: `feat:`, `fix:`, `docs:`, `chore:`, `refactor:`, `test:`, `ci:`. Breaking changes get a `!` (`feat!: …`) and a migration note in the changelog.

## Project management

This repository uses [5harness](docs/HARNESS.md) for durable work tracking (stories, decisions, intakes as git-backed markdown). You do not need harness tooling to contribute code; maintainers will record the durable entries. If you are an AI agent working in this repo, read `AGENTS.md` first.

## License

By contributing, you agree that your contributions are licensed under the [Apache License 2.0](LICENSE).

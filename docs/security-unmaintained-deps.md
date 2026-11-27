# Unmaintained Rust dependency tracking

This document tracks crates reported as unmaintained by the security scan and
the concrete migration or upstream-follow-up path for each one. Re-check this
file during every quarterly full scan and whenever a dependency touching the
same subsystem is upgraded.

| Crate | Current use | Status | Tracking action |
| --- | --- | --- | --- |
| `chrono` | Direct date/time type used across panel, agent, cache and CLI; also required by `sqlx`, `object_store`, `cron`, `chrono-tz` and Pingora. | **Blocked by ecosystem interop.** `sqlx` and `object_store` expose `chrono` types in their public APIs, so a direct migration to `time` or `jiff` would not remove the crate. | Re-evaluate when `sqlx`/`object_store` offer maintained time-type support. Until then, keep `chrono` current and monitor RUSTSEC/OSV advisories as part of incremental scans. |
| `chrono-tz` | User-facing cron schedule time-zone parsing in `dymini-panel-core`. | **Candidate for replacement.** A future slice can replace it with `jiff` TZif/database support after validating timezone-name compatibility and schedule semantics. | Open a dedicated migration before adding more timezone-dependent features; do not expand direct uses. |
| `humantime` | Transitive through `object_store`; no direct Dymini Panel source imports. | **Blocked upstream.** Removing it requires an `object_store` release that no longer depends on `humantime`. | Track `object_store` upgrades during dependency scans. |
| `derivative` | Transitive proc macro through Pingora. | **Blocked upstream.** Removing it requires Pingora to replace the derive macro. | Track Pingora upgrades during cache-proxy dependency updates. |
| `unsafe-libyaml` | Transitive YAML parser through Pingora/`serde_yaml` paths and build-time schema tooling. | **Mostly indirect/build tooling.** Production Dymini Panel code should use `serde_yaml_ng` for new YAML work. | Do not introduce new direct `serde_yaml` uses; track upstream replacement of `unsafe-libyaml`. |

## Rules for new code

- Do not add new direct uses of `chrono-tz`, `humantime`, `derivative`,
  `serde_yaml`, or `unsafe-libyaml` without documenting why no maintained
  alternative is possible.
- Prefer `serde_yaml_ng` when YAML support is required in first-party code.
- Prefer existing `chrono` types at boundaries with `sqlx`, `object_store`,
  `cron` and Pingora rather than adding lossy conversions solely to hide the
  dependency; those conversions do not remove the transitive crate.
- When upstream releases remove these crates, upgrade them promptly and remove
  the corresponding row here.

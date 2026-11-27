# Security scanning process

This repository uses an incremental, evidence-first security review process. A
finding is filed only after a human/agent verifies the reachable code path or
classifies the scanner hit as a real hardening gap. Scanner output itself is not
proof.

## Full-scan checklist

Run a full scan before a release, after a large architecture change, and when
the baseline is older than 90 days:

1. **Inventory and baseline** — record the commit, date, toolchain versions,
   changed attack surface, and excluded generated directories in
   `security/baseline.json`.
2. **Dependencies/supply chain** — scan `Cargo.lock`, `web/pnpm-lock.yaml`, and
   `app/bun.lock` with OSV-Scanner plus the native `pnpm audit` and `bun audit`.
   Determine whether each vulnerable package is production, build-only, or
   test-only before filing.
3. **Secrets** — scan working tree and Git history with Gitleaks using redacted
   output. Never paste secret values into an issue, chat, or test fixture.
   Classify intentional test keys and example placeholders explicitly in the
   baseline notes.
4. **SAST** — run Semgrep security packs over Rust, TypeScript/JavaScript,
   shell, Dockerfile, and GitHub Actions; run Trivy filesystem scanning for
   vulnerability, secret, and misconfiguration coverage.
5. **Manual high-risk review** — review authentication/sessions/MFA/CSRF,
   OAuth/MCP grants and delegated API keys, Dymini Panel Access and the edge gate,
   agent enrollment/mTLS/WebSocket tunnel, command execution and build inputs,
   archive extraction/path traversal, SSRF/webhooks, cryptographic use,
   logging/redaction, backups/restore, update verification, XSS/log rendering,
   mobile token storage/transport, Docker/install scripts/templates, and CI/CD.
6. **Configuration lint** — run zizmor, actionlint, hadolint, and shell syntax
   checks. Workflow findings are triaged for reachability from forks, secrets,
   release permissions, cache/artifact trust, and template injection.
7. **Triage and reporting** — deduplicate by root cause, record impact and
   evidence (file/line or scanner/advisory id), assign severity, and create one
   GitHub issue per independently fixable vulnerability or hardening gap. Do
   not fix in the scanning commit unless the user explicitly approved fixes.

## Incremental scans

Do not rescan every unchanged area. Start from `security/baseline.json`:

```sh
base=$(jq -r .commit security/baseline.json)
git diff --name-only "$base"...HEAD
```

Map changed paths to scan areas:

| Changed path | Required scan |
| --- | --- |
| `Cargo.toml`, `Cargo.lock`, `crates/**` | OSV/Rust dependency scan, Semgrep Rust, targeted manual review of changed trust boundary |
| `web/package.json`, `web/pnpm-lock.yaml`, `web/src/**` | pnpm audit/OSV, Semgrep TypeScript, XSS/URL/token-handling review |
| `app/package.json`, `app/bun.lock`, `app/src/**` | bun audit/OSV, Semgrep TypeScript, mobile transport/storage/deep-link review |
| `.github/workflows/**` | zizmor, actionlint, permissions/secrets/template-injection review |
| `deploy/**`, `templates/**`, `plugins/**` | Trivy/hadolint/shell checks, installer/container/template review |
| `docs/**`, `*.md` | secrets scan only, unless docs change security instructions |
| authentication/MCP/Access/agent/build/backup/update files | full manual review of that subsystem even if automated scans are clean |

A dependency-only scan is always cheap and should run even for small changes.
A full manual subsystem review is required when the change crosses a trust
boundary or handles secrets, tokens, untrusted input, archives, processes, or
network addresses.

## Local runner

`scripts/security-scan.sh` automates the non-manual portions. It expects tools
on `PATH` or under `/tmp/dymini-panel-security-tools/bin` when present:

```sh
scripts/security-scan.sh full /tmp/dymini-panel-security-scan
scripts/security-scan.sh deps /tmp/dymini-panel-security-scan
scripts/security-scan.sh config /tmp/dymini-panel-security-scan
```

The script writes raw scanner output only to the caller-selected temporary
output directory. Do not commit raw scan output: it can contain code snippets
or sensitive metadata. Commit only the process document and the redacted
baseline.

## 2026-11-27 baseline notes

The first full scan covered commit `3ff885b` plus the harness bookkeeping files
for US-135. Automated coverage completed for OSV/Trivy/Gitleaks/Semgrep,
pnpm/bun audits, zizmor, actionlint, hadolint, and shell parsing. Manual review
covered the principal authentication, Access, MCP/OAuth, installer, mobile,
crypto/redaction, archive, command-construction, and cache-admin trust paths.
Confirmed findings were filed as GitHub issues; intentional test private keys
and mock credentials were classified as false positives. `.env.example` was not
treated as a leaked real secret, but its copy-pasteable master key was filed as
a hardening issue.

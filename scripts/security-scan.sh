#!/usr/bin/env bash
# Dymini Panel security scan runner. Writes raw output only under the chosen directory.
set -euo pipefail
mode="${1:-full}"
out="${2:-/tmp/dymini-panel-security-scan}"
mkdir -p "$out"
root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
cd "$root"
tool() {
  local name="$1"
  if command -v "$name" >/dev/null 2>&1; then command -v "$name"; else printf '/tmp/dymini-panel-security-tools/bin/%s' "$name"; fi
}
run_deps() {
  echo '== dependencies =='
  "$(tool osv-scanner)" scan source -L Cargo.lock -L web/pnpm-lock.yaml -L app/bun.lock \
    --format json --output-file "$out/osv.json" --all-vulns || true
  (cd web && pnpm audit --json > "$out/pnpm-audit.json" 2>"$out/pnpm-audit.stderr") || true
  (cd web && pnpm audit --prod --json > "$out/pnpm-audit-prod.json" 2>"$out/pnpm-audit-prod.stderr") || true
  (cd app && bun audit --json > "$out/bun-audit.json" 2>"$out/bun-audit.stderr") || true
  (cd app && bun audit --prod --json > "$out/bun-audit-prod.json" 2>"$out/bun-audit-prod.stderr") || true
}
run_secrets() {
  echo '== secrets (redacted reports) =='
  "$(tool gitleaks)" git --redact --report-format json --report-path "$out/gitleaks-git.json" . || true
  "$(tool gitleaks)" dir --redact --report-format json --report-path "$out/gitleaks-dir.json" . || true
}
run_sast() {
  echo '== SAST/misconfiguration =='
  semgrep scan --config p/rust --config p/javascript --config p/typescript \
    --config p/dockerfile --config p/github-actions --config p/secrets \
    --metrics=off --json --output "$out/semgrep.json" \
    --exclude .git --exclude target --exclude node_modules --exclude web/dist \
    --exclude app/ios --exclude app/android --exclude .sqlx \
    crates web/src app/src deploy templates plugins .github || true
  "$(tool trivy)" fs . --format json --output "$out/trivy-fs.json" \
    --scanners vuln,secret,misconfig --severity CRITICAL,HIGH,MEDIUM \
    --disable-telemetry --skip-version-check --no-progress \
    --skip-dirs .git --skip-dirs target --skip-dirs node_modules \
    --skip-dirs web/dist --skip-dirs app/ios --skip-dirs app/android || true
}
run_config() {
  echo '== workflow/container/shell checks =='
  zizmor --format json --offline .github/workflows >"$out/zizmor.json" 2>"$out/zizmor.stderr" || true
  "$(tool actionlint)" -no-color .github/workflows/*.yml >"$out/actionlint.txt" 2>&1 || true
  : >"$out/hadolint.txt"
  for f in deploy/docker/*.Dockerfile; do
    echo "### $f" >>"$out/hadolint.txt"
    "$(tool hadolint)" "$f" >>"$out/hadolint.txt" 2>&1 || true
  done
  : >"$out/sh-parse.txt"
  for f in deploy/*.sh deploy/release/*.sh scripts/security-scan.sh; do
    sh -n "$f" 2>>"$out/sh-parse.txt" || echo "$f parse failed" >>"$out/sh-parse.txt"
  done
}
case "$mode" in
  deps) run_deps ;;
  secrets) run_secrets ;;
  sast) run_sast ;;
  config) run_config ;;
  full) run_deps; run_secrets; run_sast; run_config ;;
  *) echo "usage: $0 full|deps|secrets|sast|config [output-dir]" >&2; exit 2 ;;
esac
echo "scan output written under $out; review before sharing"

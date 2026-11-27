---
created_at: "2026-11-23T19:25:01.818735107+00:00"
doc: docs/decisions/0028-access-portal-cloudflare.md
id: "0028"
links:
  - US-127
notes: "Do not automatically bypass portal, login, Google callback or asset paths under the administrative panel's Cloudflare Access application. Surface a settings warning and required readiness diagnostic when the effective Dymini Panel Access portal shares the enforced panel host. Recommend a dedicated portal hostname with its own DNS/tunnel publication and updated Turnstile and Google callback settings. Do not broaden administrative API exposure."
status: accepted
title: Keep panel Cloudflare Access intact when Dymini Panel Access shares its hostname
type: decision
updated_at: "2026-11-23T19:25:01.818735746+00:00"
verify: null
---

# Keep panel Cloudflare Access intact when Dymini Panel Access shares its hostname

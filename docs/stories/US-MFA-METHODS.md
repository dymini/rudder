---
contract: null
created_at: "2026-11-22T17:44:06.637780524+00:00"
e2e: 0
evidence: "Server library 89, Access 3, auth 7, MCP 7, grants 5, OpenAPI 2 and MFA 4 tests pass. MFA tests exercise cryptographic registration/authentication using webauthn-authenticator-rs 0.5.5 SoftPasskey with test-only user-verification emulation and resident discovery adaptation, single-use/replayed/expired challenges, changing origins, stale counters, multiple keys, rename/delete, password/current-factor requirements, real local fake-SMTP delivery, email expiry/reuse/five attempts and resend/IP limits. cargo fmt --all --check, offline workspace/all-targets clippy, sqlx prepare --workspace -- --all-targets, web i18n 1803 used / 1937 translated, TypeScript and Vite build pass. Production mail, physical/browser authenticators, UI appearance and cross-musl release builds remain unverified."
id: US-MFA-METHODS
integration: 1
lane: high_risk
notes: "Existing encrypted Access mail transport reused; no new SMTP infrastructure. Required UV and resident browser enrollment; software tests emulate unsupported UV/resident-discovery features. Explicit public URL takes precedence over saved domain for external TLS termination. No existing admin require-MFA policy found. All security removals require password plus a supplied valid current factor; the first-enrollment no-factor allowance is never usable for removal, even across changing method state. Final auth 7 and MFA 4 tests pass after that guard. Initial fixture expected setup 201 instead of existing 200, corrected. Installed missing host pkg-config; vendored OpenSSL supports static release builds without workflow changes. UI, real authenticators, production mail, and release cross builds unverified."
platform: 0
status: implemented
title: Secure account sign-in with passkeys and email one-time codes
type: story
unit: 1
updated_at: "2026-11-22T18:08:40.227939357+00:00"
verify: null
---

# Secure account sign-in with passkeys and email one-time codes

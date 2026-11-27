# iOS app

Dymini Panel ships an iOS companion app (Expo) in `app/`. It controls the panel from an
iPhone instead of the browser: dashboard and server metrics, service actions
(deploy, start, stop, restart), container logs, environment variables, domains,
compose stack actions, backup jobs, and panel updates.

## Interface

The app uses a dark iOS-style control-room interface: floating rounded tab bar,
cards with depth and status color, a dashboard hero with service/server/alert
metrics, progress bars for CPU, memory and disk, and a redesigned sign-in screen
with a branded compass mark and grouped credential fields. Pull to refresh is
available on data screens.

## Signing in

The app uses the same OAuth 2.1 + PKCE flow as AI agents (see [mcp.md](mcp.md)):

1. You type the panel address.
2. The app registers a public OAuth client (`POST /oauth/register`) with the
   private redirect scheme `dymini-panel://oauth/callback`, which the panel accepts
   like any private app scheme.
3. A secure browser sheet (ASWebAuthenticationSession) opens the panel sign-in
   and consent page. Choose a permission template — **Full control** for full
   panel operation — and approve.
4. The app stores the access and refresh tokens in the iOS Keychain and refreshes
   on its own until you revoke the grant under **Admin → AI agents** or the grant
   reaches 90 days.

An API token (`rdr_...`) can be pasted instead of going through OAuth.

Every request goes to `/mcp/api/v1/...`, the machine mirror of the REST API, so
panels behind Cloudflare Access work the same way they do for the `dymini-panel` CLI
(the agent bypass must be on when Zero Trust is enforced). The MCP gate scopes
every call to the approved grant, exactly as if an AI agent were calling.

## Building

`.github/workflows/ios.yml` builds the app on `macos-latest` whenever `app/`
changes: lint and typecheck, `expo prebuild`, `pod install`, a simulator build,
and a device archive. Without signing secrets the device archive is unsigned.
With these repository secrets the workflow signs and exports a development IPA:

- `IOS_BUILD_CERTIFICATE_BASE64` — base64 of the .p12 distribution certificate
- `IOS_P12_PASSWORD` — the .p12 password
- `IOS_PROVISION_PROFILE_BASE64` — base64 of the .mobileprovision profile
- `IOS_TEAM_ID` — the Apple Developer team id

For TestFlight or App Store distribution, move to EAS Build or fastlane match;
the Expo project is standard and needs no native changes by hand.

## Developing

```bash
cd app
bun install
bunx expo start
bun run lint && bun run typecheck
```

API types in `src/lib/schema.d.ts` are generated from `web/openapi.json`
(`bun run api:types`) — regenerate after changing the panel API.

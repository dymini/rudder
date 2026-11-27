# Command line client (`dymini-panel`) & Dymini Panel Image

`dymini-panel` is a native, dependency-free Rust CLI distributed as single-file binaries for **macOS** (`x86_64` and Apple Silicon `aarch64`), **Windows** (`x86_64`), and **Linux** (`x86_64` and `aarch64` static `musl`).

In addition to managing services, logs, and cache purges, `dymini-panel` provides the **Dymini Panel Image** local CI/CD workflow: run tests and build Docker images directly on a developer's machine (or AI agent workstation), upload compressed image archives to **Cloudflare R2** through Dymini Panel, automatically prune old versions from R2 to save storage cost while keeping local archives for instant rollback, and deploy on demand or automatically.

---

## 1. Install `dymini-panel` natively

### macOS & Linux

```sh
curl -fsSL https://github.com/vantanminh/rudder/releases/latest/download/install-cli.sh | sh
```

Options for `install-cli.sh`:
- `--version v0.6.0` — install a specific release tag instead of `latest`.
- `--bin-dir ~/.local/bin` — customize the target directory (defaults to `/usr/local/bin` when writable or `~/.local/bin`).

### Windows (PowerShell)

```powershell
irm https://github.com/vantanminh/rudder/releases/latest/download/install-cli.ps1 | iex
```

The installer verifies `SHA256SUMS`, places `dymini-panel.exe` in `$HOME\.dymini-panel\bin`, and adds it to your user `PATH`.

### Native self-update

Update `dymini-panel` in place to the latest GitHub release at any time:

```sh
dymini-panel update --check   # check if a newer version is available on GitHub
dymini-panel update           # download, verify SHA-256 (and minisign when set), and replace binary in place
```

---

## 2. Sign in (`dymini-panel login`) — 3 authentication methods

`dymini-panel login` supports three ways to authenticate against your Dymini Panel. Every CLI login appears on **Account → API tokens** with its method badge (`API key`, `Browser login`, or `Device code`) and can be revoked with **Revoke** at any time.

### Method 1: API key (`DYMINI_PANEL_TOKEN` or stdin)

Create a token on **Account → API tokens** and pass it via `DYMINI_PANEL_TOKEN` or stdin (never as a command-line flag, so it stays out of shell history):

```sh
dymini-panel login --url https://panel.example.com
# API token: rdr_...
```

### Method 2: Browser authorization (`--browser`)

Starts a temporary loopback listener on `127.0.0.1`, opens your browser to the Dymini Panel, and receives the scoped token automatically once you click **Approve CLI login**:

```sh
dymini-panel login --url https://panel.example.com --browser
```

### Method 3: 8-character Device Code (`--device`)

Ideal for remote SSH sessions, containers, or headless environments. `dymini-panel` prints a verification URL and an **8-character alphanumeric code** (containing both letters and digits):

```sh
dymini-panel login --url https://panel.example.com --device
# Open this URL in your browser to authorize dymini-panel CLI:
#   https://panel.example.com/tokens?device=1
#
# Enter 8-character device code: K7M4P9X2
# Waiting for approval...
```

Open **Account → API tokens** in the Dymini Panel dashboard, enter the 8-character code under **Authorize CLI with device code**, and click **Authorize CLI**. The CLI receives its token automatically and saves it to `~/.config/dymini-panel/config.json` (mode `0600`).

`dymini-panel logout` removes the local configuration file.

---

## 3. Configure Cloudflare R2 for Dymini Panel Image

1. In the Dymini Panel web panel, open **S3 → S3 destinations**.
2. Under **New destination**, pick the **Cloudflare R2** preset, fill in your R2 endpoint (`https://<account-id>.r2.cloudflarestorage.com`), bucket name, Access Key ID, and Secret Access Key, and click **Add destination**.
3. Open **S3 → Dymini Panel Image → Setup** and, in the **Image store** card:
   - Select your Cloudflare R2 bucket as the **Bucket**.
   - Set the **Object key prefix** (default `dymini-panel-images`).
   - Optionally set **Keep on R2 (versions per service)** and **Keep on R2 (days)** so older images are automatically deleted from Cloudflare R2 to save storage cost while remaining on the developer's machine.
4. On each application or Compose service (**Services → `<app>` → General → Dymini Panel Image**), configure the per-service policy:
   - **Auto-deploy immediately when a new image is pushed**:
     - **On (default)**: `dymini-panel push` immediately loads the image from R2 into Docker on the target server and deploys it.
     - **Off**: `dymini-panel push` stores the image in R2 and emits an `image.pushed` notification without deploying until you run `dymini-panel deploy` (or click **Deploy** in the panel).
   - **Keep on Cloudflare R2 (versions)**: optional per-service override for how many versions stay in R2.

### The Dymini Panel Image page

**S3 → Dymini Panel Image** is the place to watch it all:

- **Overview**: a status checklist (bucket, retention, signed-in CLI clients, first push, deploy-on-push), a live **Check bucket** test that writes, reads, and deletes a probe object, push, build, and image-deploy counts for the last 30 days with a daily chart, usage per service, the latest pushes, and every command-line client signed in with `--browser` or `--device`.
- **Images**: every version of a service, whether it is still on the bucket, with **Deploy**, **Free bucket space** (delete from the bucket, keep the history), and **Delete**.
- **Setup**: the bucket, prefix, and retention, plus the commands to connect the CLI.

The **Overview** page of the panel also shows a small Dymini Panel Image card with the same status. The same data is available to AI agents through `GET /api/v1/image-store/overview` and `POST /api/v1/image-store/test` (MCP scope `source`).

---

## 4. Local CI/CD workflow (`dymini-panel init`, `build`, `push`, `deploy`, `images`)

Instead of spending GitHub Actions minutes or overloading your production VPS with `docker build`, you or your AI coding agent can run CI checks and build the image locally, then push the artifact to Cloudflare R2.

### Initialize `dymini-panel.toml`

Run `dymini-panel init` in your repository root:

```sh
dymini-panel init shop/prod/web
```

This creates a `dymini-panel.toml` configuration file:

```toml
# Dymini Panel project configuration (`dymini-panel build`, `dymini-panel push`, `dymini-panel deploy`)
service = "shop/prod/web"
dockerfile = "Dockerfile"
context = "."
platform = "linux/amd64"

# Commands executed locally before building the Docker image (`dymini-panel build`).
checks = [
  "cargo test",
  "pnpm build",
]

# Extra Docker --build-arg values (`GIT_SHA` and `BUILD_TIME` are injected automatically).
build_args = { APP_ENV = "production" }

# How many recent image versions to keep in Cloudflare R2 (older ones are pruned from R2).
r2_keep_count = 10

# How many recent image versions to keep on this developer machine for local rollback/re-push.
local_keep_count = 20

# When true, `dymini-panel deploy` runs `build -> push -> deploy` automatically.
deploy_build = false
```

### Step 1: Build and check locally (`dymini-panel build`)

```sh
dymini-panel build                                  # runs `checks` from dymini-panel.toml, then builds & saves image locally
dymini-panel build --tag v1.4.0                     # use an explicit version tag (defaults to YYYYMMDD-HHMMSS-<short-sha>)
dymini-panel build --platform linux/amd64           # target your server's architecture (e.g. from Apple Silicon or Windows)
dymini-panel build --no-cache --progress=plain      # force a clean BuildKit build with plain log output
dymini-panel build --build-arg FEATURE_FLAG=1       # pass additional --build-arg KEY=VAL flags (repeatable)
dymini-panel build --skip-checks                    # skip local CI check commands
```

What `dymini-panel build` does:
- Runs every command in `checks` on your machine (unless `--skip-checks` is passed).
- Measures the Docker build context (respecting `<dockerfile>.dockerignore` or `.dockerignore`) and prints a warning if the context exceeds **200 MiB**.
- Enables **BuildKit** (`DOCKER_BUILDKIT=1`) so `# syntax=docker/dockerfile:1.7` and `RUN --mount=type=cache` always work.
- Automatically passes `--build-arg GIT_SHA=<commit-or-nogit>` and `--build-arg BUILD_TIME=<RFC3339-UTC>` (plus any `build_args` from `dymini-panel.toml` or `--build-arg`). Outside a Git repository, it prints a notice and falls back to `GIT_SHA=nogit` and a timestamp tag.
- Exports and compresses the image into `~/.config/dymini-panel/images/<service>/<tag>.tar.gz` with its SHA-256 checksum, prints a summary (Image ID, Platform, Git SHA, Size, Digest, Duration, and Next step), and prunes local archives beyond `local_keep_count` (never pruning the currently deployed image).

### Step 2: Push to Cloudflare R2 (`dymini-panel push`)

```sh
dymini-panel push                         # push the most recently built local image to R2
dymini-panel push v1.4.0                  # push a specific local tag to R2
dymini-panel push --deploy                # force immediate deploy even if service auto-deploy is off
dymini-panel push --no-deploy             # upload to R2 only, even if service auto-deploy is on
dymini-panel push --r2-keep 5             # keep only the 5 newest versions on R2
```

When `dymini-panel push` runs:
- If the exact `tag` + `sha256` is already present in Cloudflare R2, `dymini-panel push` skips re-uploading the archive (and proceeds directly to deployment when `--deploy` is passed).
- Otherwise, `dymini-panel push` streams the archive with upload progress and automatic retry on transient network or `502`/`503`/`504` errors.
- Dymini Panel verifies the `expected_sha256` digest on the panel before registering the version in the database (deleting the R2 object if the digest mismatches, so a failed push never leaves a half-registered image version).
- Dymini Panel prunes older objects in R2 beyond `r2_keep_count` while protecting both the newest image and the image currently deployed on the service.
- `dymini-panel deploy [<service>] [--image TAG]` deploys the latest (or specified) version from Cloudflare R2 for both **application** and **compose** services — and automatically re-uploads a local version if it was pruned from R2. On Compose stacks, Dymini Panel injects `DYMINI_PANEL_IMAGE_TAG` and `DYMINI_PANEL_IMAGE_REF` into `.env` before running `docker compose up -d` and streams deployment logs until completion.
- If your panel is protected by **Cloudflare Access**, either add a Bypass policy for `/api/v1/*` (Dymini Panel still requires `Authorization: Bearer rdr_...` on every API endpoint) or export `CF_ACCESS_CLIENT_ID` and `CF_ACCESS_CLIENT_SECRET` from a Cloudflare Access Service Token.

---

## 5. Migrate from GitHub Actions / GHCR to local build

Moving a service from "build on GitHub Actions → push to `ghcr.io` → server pulls" to "build locally with `dymini-panel build` → push to Cloudflare R2 with `dymini-panel push` → deploy from Dymini Panel" takes four steps:

1. **Connect Cloudflare R2 once in the panel**:
   - Add your R2 bucket under **S3 → S3 destinations**, then select it under **S3 → Dymini Panel Image → Setup**.
2. **Sign in from your workstation**:
   ```sh
   dymini-panel login --url https://panel.example.com --device
   dymini-panel status --check
   ```
3. **Create `dymini-panel.toml` in your repository root**:
   ```toml
   service = "my-project/staging/api"
   dockerfile = "Dockerfile"
   context = "."
   platform = "linux/amd64"
   checks = ["cargo test --workspace"]
   deploy_build = false
   r2_keep_count = 10
   local_keep_count = 20
   ```
   - Make sure your `.dockerignore` (or `<Dockerfile>.dockerignore`) excludes large local directories such as `target/`, `node_modules/`, and `.git/` so `dymini-panel build` sends a small context to BuildKit.
   - If your `Dockerfile` declares `ARG GIT_SHA` and `ARG BUILD_TIME`, `dymini-panel build` populates both automatically on every build.
4. **Build, push, and deploy**:
   ```sh
   dymini-panel build                  # builds locally with BuildKit + layer cache
   dymini-panel push --deploy          # uploads to R2, verifies SHA-256, and deploys to staging
   ```
   Once verified on staging, you can disable or remove the `.github/workflows/*.yml` image build job and GHCR registry credentials on the server.

---

## 6. Other CLI commands

### Choosing a service

Commands take a service as its id, its name, `project/name`, or `project/environment/name` (or read `service` from `dymini-panel.toml`). `dymini-panel services` prints every service:

```text
ID                                    NAME            KIND         STATUS
6c1f…                                 shop/prod/web   application  running
91aa…                                 shop/prod/blog  compose      running
```

### Logs

```sh
dymini-panel logs shop/web                       # last 100 lines, oldest first
dymini-panel logs shop/web -n 500 --since 2h
dymini-panel logs shop/web --level error,warn --source app --grep timeout
dymini-panel logs shop/web -f                    # keep following
```

### Purge the cache

```sh
dymini-panel cache purge shop/web --all
dymini-panel cache purge shop/web --path /index.html --path /app.css
dymini-panel cache purge shop/web --prefix /blog/ [--domain <domain id>]
```

### Decrypt a backup

```sh
DYMINI_PANEL_BACKUP_PASSPHRASE=... dymini-panel backup decrypt --in db.dump.zst.enc --out db.dump.zst
```

---

## 7. Skills for AI agents

`dymini-panel skills` installs a skill, a short guide that AI coding agents load on demand, so they know what Dymini Panel is and how to use it: the `dymini-panel` CLI, the MCP server, the permission model, and when to ask you before doing something destructive. The text is built into the CLI, so it always matches the version you installed.

```sh
dymini-panel skills install                       # every agent found in your home directory
dymini-panel skills install --agent claude        # one agent
dymini-panel skills install --project             # into this repository, so the whole team gets it
dymini-panel skills list                          # what is installed and whether it is current
dymini-panel skills remove
dymini-panel skills print                         # the skill text, to put anywhere else
```

| `--agent` | Your home directory | With `--project` |
| --- | --- | --- |
| `claude` | `~/.claude/skills/dymini-panel/SKILL.md` | `.claude/skills/dymini-panel/SKILL.md` |
| `codex` | `$CODEX_HOME` or `~/.codex`, then `skills/dymini-panel/SKILL.md` | `.agents/skills/dymini-panel/SKILL.md` |
| `agents` | `~/.agents/skills/dymini-panel/SKILL.md` | `.agents/skills/dymini-panel/SKILL.md` |
| `cursor` | not available | `.cursor/rules/dymini-panel.mdc` |

`--dir DIR` writes `DIR/dymini-panel/SKILL.md` for any other agent that reads skill folders. Without `--agent`, the agents whose folder exists are used, and the command says so when it finds none.

Files written by `dymini-panel skills` carry a marker. Running `install` again updates them after you upgrade the CLI. A file with the same name that `dymini-panel` did not write is left alone unless you pass `--force`, and `remove` only deletes files it wrote. `--dry-run` shows what would change. Restart the agent afterwards so it loads the skill.

### Documentation from your panel

The skill sends agents to your panel for documentation, so they read the version you run:

```sh
dymini-panel docs search deploy an image          # ranked sections with snippets
dymini-panel docs read cli                        # a whole guide
dymini-panel docs list
```

Agents connected over MCP use the same content through `get_v1_docs_search` and `get_v1_docs__name_` (see [Search the documentation](mcp.md#search-the-documentation)).

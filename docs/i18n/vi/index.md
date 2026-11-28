---
translatedFrom: fa783243fa28
---

# Dymini Panel

[![CI](https://github.com/dymini/panel/actions/workflows/ci.yml/badge.svg)](https://github.com/dymini/panel/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
[![Release](https://img.shields.io/github/v/release/dymini/panel?sort=semver)](https://github.com/dymini/panel/releases)

Dymini Panel là nền tảng triển khai tự host, gọn nhẹ, thay thế Dokploy hay Coolify cho những ai muốn một VPS chạy mọi thứ. Một máy chủ chạy panel điều khiển. Các máy chủ khác chạy một agent nhỏ, tự kết nối ra panel và được quản lý từ cùng một bảng điều khiển. Không có bộ lập lịch cụm: mỗi máy chủ chạy độc lập.

Bạn có:

- **Ứng dụng** từ container image (kể cả registry riêng như GHCR) hoặc từ Git với Dockerfile, Nixpacks hay bản build tĩnh; webhook khi push (cài GitHub App bằng một cú bấm), deploy có kiểm tra sức khỏe và rollback.
- **Tên miền và TLS** qua Traefik v3 (Let's Encrypt hoặc chứng chỉ tải lên), thêm HTTP cache dựa trên Pingora cho từng máy chủ nếu muốn.
- **Database**: Postgres, MySQL, MariaDB, MongoDB và Redis, sao lưu mã hóa dạng luồng tới bất kỳ kho S3 tương thích nào.
- **Docker Compose stack** và 11 template cài một cú bấm (n8n, Uptime Kuma, Gitea, Vaultwarden, Ghost, WordPress…).
- **Vận hành**: log tập trung xem trực tiếp, terminal trên web, số liệu và cảnh báo (Telegram, Discord, Slack, webhook, email), bộ chống quá tải giữ panel truy cập được khi build hay lưu lượng làm máy chủ quá tải, sao lưu panel và khôi phục sau thảm họa, tự cập nhật có chữ ký và rollback.
- **Truy cập**: người dùng với vai trò admin và viewer, TOTP, API token có phạm vi, nhật ký kiểm toán, tài liệu OpenAPI ở `/api/docs` và CLI `dymini-panel`.

## Bắt đầu nhanh

Trên một VPS Linux mới, mở các cổng 80, 443, 3000 và 7443:

```sh
curl -fsSL https://github.com/dymini/panel/releases/latest/download/install.sh | sudo sh
```

Trình cài đặt tự cài Docker nếu chưa có. Nó chạy Postgres, Traefik, `dymini-panel-cache` và panel dưới dạng container, rồi in ra link thiết lập kèm token thiết lập dùng một lần (cũng lưu ở `/var/lib/dymini-panel/setup-token`). Mở link đó và tạo tài khoản chủ sở hữu. Thêm `--domain panel.example.com --email you@example.com` để panel chạy qua HTTPS. Xem `install.sh --help` cho các tùy chọn sao lưu và khôi phục, và [khôi phục sau thảm họa](docs/disaster-recovery.md) khi chuyển sang máy chủ mới.

Để thêm máy chủ thứ hai, vào **Servers → Add server** rồi chạy lệnh tham gia hiện ra trên máy đó:

```sh
curl -fsSL http://<panel>:3000/install-agent.sh | sudo sh -s -- --server http://<panel>:3000 --token join_...
```

Agent đăng ký bằng một token dùng một lần, sau đó giữ một phiên gRPC đi ra (mutual TLS) tới panel ở cổng 7443. Máy chủ không cần mở cổng vào nào ngoài 80/443 cho ứng dụng của chính nó. Sau đó mọi trang trong panel đều có thể chọn máy chủ này. Xem [agent](docs/agent.md).

Từ laptop hoặc CI: cài CLI `dymini-panel` trên macOS/Linux (`curl -fsSL https://github.com/dymini/panel/releases/latest/download/install-cli.sh | sh`) hoặc Windows PowerShell (`irm https://github.com/dymini/panel/releases/latest/download/install-cli.ps1 | iex`), đăng nhập bằng API key, trình duyệt hoặc mã thiết bị 8 ký tự, rồi chạy CI/CD ngay trên máy và đẩy image lên Cloudflare R2 (xem [CLI](docs/cli.md)):

```sh
dymini-panel login --url https://panel.example.com --device  # hoặc --browser, hoặc dán API token
dymini-panel init shop/prod/web
dymini-panel build && dymini-panel push                            # kiểm tra CI trên máy -> build -> đẩy lên Cloudflare R2
dymini-panel deploy                                          # deploy từ R2 (hoặc `dymini-panel deploy --build`)
dymini-panel images list                                     # quản lý các phiên bản image trên máy và trên R2
dymini-panel update                                          # tự cập nhật từ GitHub Releases
dymini-panel deploy shop/web --image ghcr.io/acme/web:1.4.2
dymini-panel logs shop/web -f
dymini-panel cache purge shop/web --all
```

## Tài nguyên sử dụng

Đây là số đo thật, không phải ước tính. Chúng lấy từ bản phát hành musl x86_64 trên VM Ubuntu 24.04 4 vCPU, 16 GB, Postgres 16 chạy cùng máy. Cách đo nằm ở cuối mục này.

| Tiến trình                                                 | RSS khi rảnh             | Mục tiêu | Khởi động                                                                                  |
| ---------------------------------------------------------- | ------------------------ | -------- | ------------------------------------------------------------------------------------------ |
| `dymini-panel-server` (control plane, nhúng giao diện web) | 27,3 MiB (đỉnh 42,6 MiB) | < 60 MB  | 38–44 ms tới `/api/health` (khởi động ấm); 402 ms với database trống, gồm cả mọi migration |
| `dymini-panel-agent`                                       | 9,1 MiB                  | < 15 MB  | 85 ms tới khi phiên kết nối                                                                |
| `dymini-panel-cache`                                       | 5,0 MiB                  | < 30 MB  | 7 ms tới `/health`                                                                         |

- **Khi rảnh:** 60 giây sau khi khởi động, một agent đang kết nối, panel lấy mẫu Docker cục bộ mỗi 10 giây và cache chạy với bộ luật trống. Giá trị là `VmRSS` và `VmHWM` trong `/proc/<pid>/status`.
- **Chạy lâu:** trong bản cài dạng container dùng cho buổi diễn tập cập nhật, sau khoảng 35 phút với một agent kết nối và một ứng dụng đã deploy, `dymini-panel-server` trong container panel dùng 23,2 MiB, `dymini-panel-cache` 5,2 MiB và `dymini-panel-agent` ở máy xa 8,6 MiB.
- **Bản cài mới sau khi đi hết các bước:** `docker stats` (bộ nhớ cgroup) cho thấy container panel `dymini-panel` dùng 22,7 MiB và mỗi container `dymini-panel-cache` 1,5 MiB, Traefik 26,8 MiB và Postgres 69,0 MiB. Agent của máy chủ thứ hai có `VmRSS` 10,1 MiB.
- **Kích thước file chạy:** `dymini-panel-server` 38 MB, `dymini-panel-agent` 12 MB, `dymini-panel-cache` 8,4 MB, `dymini-panel` 6,6 MB.

Bảng không tính Postgres, Traefik hay ứng dụng của bạn.

<details>
<summary>Cách lấy các số đo</summary>

1. Build các file chạy bằng `SQLX_OFFLINE=true cargo build --release --target x86_64-unknown-linux-musl -p dymini-panel-server -p dymini-panel-agent -p dymini-panel-cache`.
2. Chạy panel với database mới và đo thời gian tới khi `/api/health` trả lời. Khởi động lại năm lần và đo từng lần khởi động ấm.
3. Tạo tài khoản chủ sở hữu và một máy chủ xa, rồi đăng ký và chạy agent.
4. Chạy `dymini-panel-cache --rules rules.json` và đợi 60 giây trước khi đọc `/proc/<pid>/status`.

</details>

## Tài liệu

Đọc các hướng dẫn trực tuyến, theo từng bản phát hành và bằng tiếng Anh hoặc tiếng Việt, tại [developers.dymini.com/panel](https://developers.dymini.com/panel).

| Chủ đề                                  | Tài liệu                                |
| --------------------------------------- | --------------------------------------- |
| Máy chủ xa và giao thức agent           | [Agent](docs/agent.md)                  |
| Database và sao lưu                     | [Sao lưu](docs/backups.md)              |
| Compose stack và template               | [Stack](docs/stacks.md)                 |
| GitHub App và deploy khi push           | [GitHub App](docs/github-app.md)        |
| HTTP cache                              | [Cache](docs/cache.md)                  |
| Log tập trung và terminal               | [Log](docs/logs.md)                     |
| Giám sát và cảnh báo                    | [Giám sát](docs/monitoring.md)          |
| Chống quá tải                           | [Load guard](docs/load-guard.md)        |
| Sao lưu panel và khôi phục sau thảm họa | [Khôi phục](docs/disaster-recovery.md)  |
| Tự cập nhật và phát hành                | [Cập nhật](docs/updates.md)             |
| CLI                                     | [CLI](docs/cli.md)                      |
| Kiến trúc                               | [ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Quyết định thiết kế                     | [docs/decisions/](docs/decisions/)      |

API được mô tả bằng tài liệu OpenAPI phục vụ tại `/api/openapi.json`, có trình xem ở `/api/docs`. Kiểu dữ liệu của giao diện web sinh ra từ cùng tài liệu đó (`web/openapi.json`).

## Workspace

| Đường dẫn       | Vai trò                                                                |
| --------------- | ---------------------------------------------------------------------- |
| `crates/core`   | Kiểu miền, kiểm tra hợp lệ, xác minh bản phát hành, trait `NodeDriver` |
| `crates/node`   | Driver Docker dùng chung cho panel và agent                            |
| `crates/proto`  | Protobuf của phiên agent                                               |
| `crates/server` | Control plane (Axum, PostgreSQL, nhúng giao diện web)                  |
| `crates/agent`  | Agent trên máy chủ                                                     |
| `crates/cache`  | HTTP cache Pingora                                                     |
| `crates/cli`    | CLI `dymini-panel`                                                     |
| `web`           | Panel React (React Router 7, TanStack Query, Tailwind)                 |
| `deploy`        | `install.sh`, `install-agent.sh`, Dockerfile, công cụ phát hành        |

## Phát triển

Bạn cần Rust 1.88, pnpm 10, Node 22, PostgreSQL 16 và Docker. Chép `.env.example` và export các biến, rồi:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --workspace
pnpm --dir web install
pnpm --dir web build
```

Các test khởi động container thật được bỏ qua trừ khi đặt `DYMINI_PANEL_DOCKER_TESTS=1`.

SQL được kiểm tra lúc biên dịch bằng `sqlx`. CI đặt `SQLX_OFFLINE=true` và đọc cache `.sqlx` đã commit. Sau khi đổi một truy vấn, áp dụng migration và làm mới cache:

```bash
DATABASE_URL=postgres://dymini-panel:dymini-panel@127.0.0.1:5432/dymini-panel cargo sqlx prepare --workspace -- --all-targets
```

Sau khi đổi API, sinh lại ảnh chụp OpenAPI và kiểu dữ liệu của frontend. Có test fail khi một trong hai bị cũ hoặc khi một route thiếu trong tài liệu.

```bash
cargo run -p dymini-panel-server -- openapi > web/openapi.json && pnpm --dir web gen:api
```

Workflow `CI` chạy các bước kiểm tra này ở mỗi lần push và pull request. Đẩy một tag `v*` sẽ chạy workflow `Release`: build file chạy musl cho x86_64 và aarch64 cùng image đa kiến trúc, ký `SHA256SUMS` bằng minisign rồi đăng bản phát hành (xem [cập nhật](docs/updates.md)).

## Trạng thái và giới hạn

Dymini Panel nhắm tới vài máy chủ cho mỗi panel. Không có Kubernetes, Swarm, cụm hay sẵn sàng cao, tính tiền hay tổ chức nhiều tenant.

Đã kiểm chứng từ đầu đến cuối trên máy Docker-in-Docker:

- `install.sh`, admin đầu tiên, `dymini-panel login`/`deploy`/`logs`/`cache purge`, một lần deploy có tên miền và HTTP cache, và một máy chủ thứ hai tham gia bằng `install-agent.sh` phục vụ deploy và cache của chính nó (systemd được thay bằng bản giả chạy `ExecStart` của unit),
- sao lưu và khôi phục sang máy chủ mới,
- tự cập nhật có chữ ký với rollback.

Chưa kiểm chứng:

- chính workflow phát hành trên GitHub (chạy ở tag đầu tiên),
- bản build arm64,
- khởi động lại systemd thật,
- Let's Encrypt với tên miền công khai,
- gửi thông báo tới Telegram, Slack, Discord hoặc nhà cung cấp SMTP thật.

Lịch sử PR có đầy đủ chi tiết.

Panel có tiếng Anh và tiếng Việt; mỗi người tự chọn ở trang Account. Chữ giao diện viết bằng tiếng Anh trong các lời gọi `t("…")` và được dịch trong `web/src/i18n/vi.ts`, khóa theo chính câu tiếng Anh. Sau khi thêm hoặc đổi chữ giao diện, thêm bản tiếng Việt và chạy `pnpm --dir web check:i18n`; lệnh này fail khi thiếu một mục hoặc khi placeholder giữa hai ngôn ngữ khác nhau.

## Đóng góp

Mọi đóng góp đều được chào đón: báo lỗi, đề xuất tính năng, cải thiện tài liệu và code. Đọc [CONTRIBUTING.md](CONTRIBUTING.md) để biết cách dựng môi trường phát triển và một pull request cần gì, và hãy tuân theo [quy tắc ứng xử](CODE_OF_CONDUCT.md). Câu hỏi và ý tưởng đặt ở [discussions](https://github.com/dymini/panel/discussions).

## Bảo mật

Để báo lỗ hổng, dùng [báo cáo lỗ hổng riêng tư của GitHub](https://github.com/dymini/panel/security/advisories/new), không bao giờ mở issue công khai. Xem [SECURITY.md](SECURITY.md) về chính sách, phạm vi và ghi chú gia cố.

## Giấy phép

[Apache License 2.0](LICENSE), bản quyền thuộc những người đóng góp cho Dymini. Giấy phép Apache chỉ áp dụng cho code, không cấp quyền dùng tên hay logo Dymini.

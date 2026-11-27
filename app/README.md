# Dymini Panel iOS app

Ứng dụng iOS (Expo) điều khiển panel Dymini Panel từ điện thoại: xem trạng thái máy chủ
và dịch vụ, triển khai / khởi động / dừng / khởi động lại dịch vụ, đọc logs, sửa
biến môi trường, quản lý tên miền, thao tác compose stack, chạy backup và cập nhật
panel.

## Đăng nhập

App dùng OAuth 2.1 + PKCE của panel — cùng flow mà AI agents (MCP) dùng:

1. Nhập địa chỉ panel (ví dụ `https://dymini-panel.example.com`).
2. App đăng ký OAuth client động (`POST /oauth/register`) với redirect scheme
   riêng `dymini-panel://oauth/callback`.
3. Một phiên trình duyệt bảo mật (ASWebAuthenticationSession) mở trang đăng nhập
   của panel; bạn đăng nhập (passkey / mật khẩu + MFA) và duyệt quyền.
4. App nhận access token (15 phút) + refresh token (30 ngày, tự xoay vòng), lưu
   trong iOS Keychain qua expo-secure-store.

Mọi API call đi qua tiền tố `/mcp/api/v1/...` — cùng API `/api/v1` nhưng qua được
Cloudflare Access edge bypass giống `dymini-panel` CLI. Quyền của app bị chặn bởi grant
đã duyệt (chọn **Full control** khi consent để điều khiển toàn bộ). Thu hồi bất cứ
lúc nào ở **Admin → AI agents** trên panel.

Có tùy chọn đăng nhập bằng API token (`rdr_...`) thay vì OAuth.

## Phát triển

```bash
bun install
bunx expo start          # dev server
bun run lint             # eslint
bun run typecheck        # tsc --noEmit
bun run api:types        # sinh lại src/lib/schema.d.ts từ ../web/openapi.json
```

## Build trên CI

`.github/workflows/ios.yml` chạy trên `macos-latest` khi `app/` thay đổi:

- lint + typecheck,
- `expo prebuild` sinh project Xcode, `pod install`,
- build simulator (không ký) — artifact `dymini-panel-simulator-app`,
- archive cho thiết bị thật: không có secrets thì archive **unsigned**; nếu repo có
  secrets `IOS_BUILD_CERTIFICATE_BASE64`, `IOS_P12_PASSWORD`,
  `IOS_PROVISION_PROFILE_BASE64`, `IOS_TEAM_ID` thì ký và xuất IPA (development
  method) để cài qua Apple Configurator / TestFlight nội bộ.

Để phát hành App Store / TestFlight chính thức, nên chuyển sang EAS Build hoặc
fastlane match — xem thêm https://docs.expo.dev.

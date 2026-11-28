import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { router } from "expo-router";
import { useState } from "react";
import { Alert, StyleSheet, Text, View } from "react-native";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  ErrorBox,
  GroupedRow,
  InsetGroup,
  Loading,
  Row,
  Screen,
  SectionTitle,
} from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { colors, spacing, statusTone } from "../../lib/theme";
import {
  formatTime,
  timeAgo,
  type AlertView,
  type McpGrantView,
  type TokenView,
  type UpdateOverview,
  type UserView,
} from "../../lib/types";

interface SessionResponse {
  user: UserView;
}

export default function System() {
  const { api, signOut, panelUrl } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);

  const me = useQuery({ queryKey: ["me"], queryFn: () => api<SessionResponse>("/api/v1/me") });
  const updates = useQuery({
    queryKey: ["updates"],
    queryFn: () => api<UpdateOverview>("/api/v1/system/updates"),
  });
  const alerts = useQuery({
    queryKey: ["alerts"],
    queryFn: () => api<AlertView[]>("/api/v1/alerts"),
  });
  const agents = useQuery({
    queryKey: ["mcp-grants"],
    queryFn: () => api<McpGrantView[]>("/api/v1/mcp/grants"),
  });
  const tokens = useQuery({
    queryKey: ["tokens"],
    queryFn: () => api<TokenView[]>("/api/v1/tokens"),
  });

  const check = useMutation({
    mutationFn: () => api("/api/v1/system/updates/check", { method: "POST", body: "{}" }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["updates"] }),
    onError: (err) => setError(err),
  });

  const apply = useMutation({
    mutationFn: () => api("/api/v1/system/updates/apply", { method: "POST", body: "{}" }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["updates"] }),
    onError: (err) => setError(err),
  });

  function confirmUpdate() {
    Alert.alert("Cập nhật panel", "Panel sẽ cập nhật và có thể gián đoạn vài phút. Tiếp tục?", [
      { text: "Hủy", style: "cancel" },
      { text: "Cập nhật", onPress: () => apply.mutate() },
    ]);
  }

  function confirmSignOut() {
    Alert.alert("Đăng xuất", "Thu hồi phiên đăng nhập trên thiết bị này?", [
      { text: "Hủy", style: "cancel" },
      {
        text: "Đăng xuất",
        style: "destructive",
        onPress: async () => {
          await signOut();
        },
      },
    ]);
  }

  const firingAlerts = (alerts.data ?? []).filter((a) => a.state === "firing");

  return (
    <Screen
      refreshing={me.isRefetching}
      onRefresh={() => {
        me.refetch();
        updates.refetch();
        alerts.refetch();
        agents.refetch();
        tokens.refetch();
      }}
    >
      <ErrorBox error={error ?? updates.error} />

      {/* Surface firing alerts at the very top */}
      {firingAlerts.length > 0 ? (
        <>
          <SectionTitle>Cảnh báo đang kích hoạt</SectionTitle>
          <InsetGroup style={{ borderColor: colors.danger + "44" }}>
            {firingAlerts.map((alert) => (
              <GroupedRow
                key={alert.id}
                icon="notifications-outline"
                tone="danger"
                title={alert.name}
                subtitle={`Từ ${formatTime(alert.state_since)}`}
                right={<Badge label={alert.state} tone={statusTone(alert.state)} />}
              />
            ))}
          </InsetGroup>
        </>
      ) : null}

      {/* Group 1: Infrastructure */}
      <SectionTitle>Hạ tầng & triển khai</SectionTitle>
      <InsetGroup>
        <GroupedRow icon="folder-outline" title="Dự án" subtitle="Project và môi trường" onPress={() => router.push("/projects")} />
        <GroupedRow icon="server-outline" title="Máy chủ" subtitle="Máy chủ chạy dịch vụ" onPress={() => router.push("/servers")} />
        <GroupedRow icon="hammer-outline" title="Builders" subtitle="Build host và bộ nhớ đệm" onPress={() => router.push("/builders")} />
        <GroupedRow icon="layers-outline" title="Templates" subtitle="Triển khai mẫu một chạm" onPress={() => router.push("/templates")} />
        <GroupedRow icon="extension-puzzle-outline" title="Plugins" subtitle="Tiện ích mở rộng cộng đồng" onPress={() => router.push("/plugins")} />
      </InsetGroup>

      {/* Group 2: Observability */}
      <SectionTitle>Giám sát & nhật ký</SectionTitle>
      <InsetGroup>
        <GroupedRow icon="speedometer-outline" title="Tải hệ thống" subtitle="Load guard và hàng đợi build" onPress={() => router.push("/load")} />
        <GroupedRow icon="stats-chart-outline" title="Thống kê" subtitle="Lưu lượng, tài nguyên và vận hành" onPress={() => router.push("/statistics")} />
        <GroupedRow icon="terminal-outline" title="Nhật ký panel" subtitle="Logs ứng dụng và proxy" onPress={() => router.push("/logs")} />
        <GroupedRow icon="time-outline" title="Nhật ký kiểm toán" subtitle="Lịch sử thao tác hệ thống" onPress={() => router.push("/audit")} />
      </InsetGroup>

      {/* Group 3: Network & Storage */}
      <SectionTitle>Mạng, bảo mật & lưu trữ</SectionTitle>
      <InsetGroup>
        <GroupedRow icon="globe-outline" title="Tên miền" subtitle="Domains và định tuyến" onPress={() => router.push("/domains")} />
        <GroupedRow icon="lock-closed-outline" title="Chứng chỉ TLS" subtitle="Certificates và gia hạn" onPress={() => router.push("/certificates")} />
        <GroupedRow icon="flash-outline" title="Cache edge" subtitle="Chính sách bộ nhớ đệm" onPress={() => router.push("/cache-policies")} />
        <GroupedRow icon="cloud-outline" title="Cloudflare" subtitle="Tunnel và DNS zones" onPress={() => router.push("/cloudflare")} />
        <GroupedRow icon="shield-checkmark-outline" title="Bảo mật" subtitle="Findings và kiểm tra SSL" onPress={() => router.push("/security")} />
        <GroupedRow icon="archive-outline" title="S3 destinations" subtitle="Kho lưu trữ sao lưu" onPress={() => router.push("/s3-destinations")} />
        <GroupedRow icon="cube-outline" title="Dymini Panel Image" subtitle="Kho image nội bộ" onPress={() => router.push("/image-store")} />
      </InsetGroup>

      {/* Group 4: Integrations & Admin */}
      <SectionTitle>Tích hợp & quản trị</SectionTitle>
      <InsetGroup>
        <GroupedRow icon="git-branch-outline" title="Git providers" subtitle="GitHub / GitLab đã kết nối" onPress={() => router.push("/git-providers")} />
        <GroupedRow icon="albums-outline" title="Registries" subtitle="Docker registries" onPress={() => router.push("/registries")} />
        <GroupedRow icon="people-outline" title="Người dùng" subtitle="Tài khoản quản trị panel" onPress={() => router.push("/users")} />
        <GroupedRow icon="key-outline" title="Dymini Panel Access" subtitle="Định danh và phân quyền" onPress={() => router.push("/access")} />
      </InsetGroup>

      {/* Account & Panel Updates */}
      <SectionTitle>Phiên đăng nhập & cập nhật</SectionTitle>
      <Card>
        <CardTitle>Tài khoản</CardTitle>
        <Row label="Email" value={me.data?.user.email ?? "—"} />
        <Row label="Quyền" value={me.data?.user.role ?? "—"} dim />
        <Row label="Panel" value={panelUrl ?? "—"} dim mono />
      </Card>

      {updates.isLoading ? (
        <Loading />
      ) : updates.data ? (
        <Card>
          <CardTitle>Cập nhật hệ thống</CardTitle>
          <Row label="Đang chạy" value={updates.data.current_version} mono />
          {updates.data.last_check?.latest ? (
            <Row
              label="Bản mới nhất"
              value={
                <Badge
                  label={updates.data.last_check.latest.version}
                  tone={
                    updates.data.last_check.latest.version !== updates.data.current_version
                      ? "warn"
                      : "ok"
                  }
                />
              }
            />
          ) : null}
          {updates.data.last_check?.checked_at ? (
            <Row label="Kiểm tra lúc" value={timeAgo(updates.data.last_check.checked_at)} dim />
          ) : null}
          {updates.data.apply_blocker ? (
            <Row label="Bị chặn" value={updates.data.apply_blocker} />
          ) : null}
          <View style={styles.actions}>
            <Button
              title="Kiểm tra"
              small
              tone="ghost"
              loading={check.isPending}
              onPress={() => check.mutate()}
            />
            <Button
              title="Cập nhật ngay"
              small
              loading={apply.isPending}
              disabled={!updates.data.last_check?.pending}
              onPress={confirmUpdate}
            />
          </View>
        </Card>
      ) : null}

      <SectionTitle>AI agents đã duyệt</SectionTitle>
      {(agents.data ?? []).length === 0 ? (
        <Text style={styles.dim}>Chưa có agent nào.</Text>
      ) : (
        <InsetGroup>
          {(agents.data ?? []).map((grant) => (
            <GroupedRow
              key={grant.id}
              title={grant.name || grant.client_name}
              subtitle={`${grant.template}${grant.last_used_at ? ` · ${timeAgo(grant.last_used_at)}` : ""}`}
            />
          ))}
        </InsetGroup>
      )}

      <SectionTitle>API tokens</SectionTitle>
      {(tokens.data ?? []).length === 0 ? (
        <Text style={styles.dim}>Chưa có token nào.</Text>
      ) : (
        <InsetGroup>
          {(tokens.data ?? []).map((token) => (
            <GroupedRow
              key={token.id}
              title={token.name}
              subtitle={
                [
                  token.last_used_at ? `Dùng cuối ${timeAgo(token.last_used_at)}` : null,
                  token.expires_at ? `Hết hạn ${formatTime(token.expires_at)}` : null,
                ]
                  .filter(Boolean)
                  .join(" · ") || "Hoạt động"
              }
            />
          ))}
        </InsetGroup>
      )}

      <View style={{ marginTop: spacing.sm }}>
        <Button title="Đăng xuất" tone="danger" onPress={confirmSignOut} />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  actions: { flexDirection: "row", gap: spacing.sm, marginTop: spacing.xs },
  dim: { color: colors.textDim, fontSize: 13, marginLeft: 4 },
});

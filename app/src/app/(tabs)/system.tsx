import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert, StyleSheet, Text, View } from "react-native";
import { router } from "expo-router";
import { Badge, Button, Card, CardTitle, ErrorBox, ListItem, Loading, Row, Screen, SectionTitle } from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { colors, spacing, statusTone } from "../../lib/theme";
import { formatTime, timeAgo, type AlertView, type McpGrantView, type TokenView, type UpdateOverview, type UserView } from "../../lib/types";

interface SessionResponse {
  user: UserView;
}

export default function System() {
  const { api, signOut, panelUrl } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);

  const me = useQuery({ queryKey: ["me"], queryFn: () => api<SessionResponse>("/api/v1/me") });
  const updates = useQuery({ queryKey: ["updates"], queryFn: () => api<UpdateOverview>("/api/v1/system/updates") });
  const alerts = useQuery({ queryKey: ["alerts"], queryFn: () => api<AlertView[]>("/api/v1/alerts") });
  const agents = useQuery({ queryKey: ["mcp-grants"], queryFn: () => api<McpGrantView[]>("/api/v1/mcp/grants") });
  const tokens = useQuery({ queryKey: ["tokens"], queryFn: () => api<TokenView[]>("/api/v1/tokens") });

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

      <SectionTitle>Quản trị</SectionTitle>
      <ListItem title="Projects" subtitle="Project và môi trường" onPress={() => router.push("/projects")} />
      <ListItem title="Servers" subtitle="Máy chủ chạy dịch vụ" onPress={() => router.push("/servers")} />
      <ListItem title="Builders" subtitle="Builder và build cache" onPress={() => router.push("/builders")} />
      <ListItem title="Tải hệ thống" subtitle="Load guard và hàng đợi build" onPress={() => router.push("/load")} />
      <ListItem title="Thống kê" subtitle="Lưu lượng và vận hành" onPress={() => router.push("/statistics")} />
      <ListItem title="Logs panel" subtitle="Nhật ký của panel" onPress={() => router.push("/logs")} />
      <ListItem title="Tên miền" subtitle="Domains và routing" onPress={() => router.push("/domains")} />
      <ListItem title="Chứng chỉ TLS" subtitle="Certificates" onPress={() => router.push("/certificates")} />
      <ListItem title="Registries" subtitle="Docker registries" onPress={() => router.push("/registries")} />
      <ListItem title="Git providers" subtitle="GitHub/GitLab đã kết nối" onPress={() => router.push("/git-providers")} />
      <ListItem title="Rudder Image" subtitle="Kho image nội bộ" onPress={() => router.push("/image-store")} />
      <ListItem title="Cache edge" subtitle="Cache policies" onPress={() => router.push("/cache-policies")} />
      <ListItem title="S3 destinations" subtitle="Nơi lưu backup" onPress={() => router.push("/s3-destinations")} />
      <ListItem title="Templates" subtitle="Template một chạm" onPress={() => router.push("/templates")} />
      <ListItem title="Plugins" subtitle="Plugin cộng đồng" onPress={() => router.push("/plugins")} />
      <ListItem title="Người dùng" subtitle="Tài khoản panel" onPress={() => router.push("/users")} />
      <ListItem title="Rudder Access" subtitle="Định danh và phân quyền" onPress={() => router.push("/access")} />
      <ListItem title="Bảo mật" subtitle="Findings và SSL" onPress={() => router.push("/security")} />
      <ListItem title="Cloudflare" subtitle="Tunnel và edge" onPress={() => router.push("/cloudflare")} />
      <ListItem title="Nhật ký audit" subtitle="Ai đã làm gì" onPress={() => router.push("/audit")} />


      <Card>
        <CardTitle>Tài khoản</CardTitle>
        <Row label="Email" value={me.data?.user.email ?? "—"} />
        <Row label="Quyền" value={me.data?.user.role ?? "—"} dim />
        <Row label="Panel" value={panelUrl ?? "—"} dim />
      </Card>

      {updates.isLoading ? (
        <Loading />
      ) : updates.data ? (
        <Card>
          <CardTitle>Cập nhật</CardTitle>
          <Row label="Đang chạy" value={updates.data.current_version} />
          {updates.data.last_check?.latest ? (
            <Row
              label="Bản mới nhất"
              value={
                <Badge
                  label={updates.data.last_check.latest.version}
                  tone={updates.data.last_check.latest.version !== updates.data.current_version ? "warn" : "ok"}
                />
              }
            />
          ) : null}
          {updates.data.last_check?.checked_at ? (
            <Row label="Kiểm tra lúc" value={timeAgo(updates.data.last_check.checked_at)} dim />
          ) : null}
          {updates.data.apply_blocker ? <Row label="Bị chặn" value={updates.data.apply_blocker} /> : null}
          <View style={styles.actions}>
            <Button title="Kiểm tra" small tone="ghost" loading={check.isPending} onPress={() => check.mutate()} />
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

      {firingAlerts.length > 0 ? (
        <>
          <SectionTitle>Cảnh báo đang kích hoạt</SectionTitle>
          {firingAlerts.map((alert) => (
            <Card key={alert.id}>
              <Row label="Tên" value={alert.name} />
              <Row label="Từ" value={formatTime(alert.state_since)} dim />
              <Row label="Trạng thái" value={<Badge label={alert.state} tone={statusTone(alert.state)} />} />
            </Card>
          ))}
        </>
      ) : null}

      <SectionTitle>AI agents đã duyệt</SectionTitle>
      {(agents.data ?? []).length === 0 ? (
        <Text style={styles.dim}>Chưa có agent nào.</Text>
      ) : (
        (agents.data ?? []).map((grant) => (
          <Card key={grant.id}>
            <Row label="Tên" value={grant.name || grant.client_name} />
            <Row label="Template" value={grant.template} dim />
            {grant.last_used_at ? <Row label="Dùng cuối" value={timeAgo(grant.last_used_at)} dim /> : null}
          </Card>
        ))
      )}

      <SectionTitle>API tokens</SectionTitle>
      {(tokens.data ?? []).length === 0 ? (
        <Text style={styles.dim}>Chưa có token nào.</Text>
      ) : (
        (tokens.data ?? []).map((token) => (
          <Card key={token.id}>
            <Row label="Tên" value={token.name} />
            {token.last_used_at ? <Row label="Dùng cuối" value={timeAgo(token.last_used_at)} dim /> : null}
            {token.expires_at ? <Row label="Hết hạn" value={formatTime(token.expires_at)} dim /> : null}
          </Card>
        ))
      )}

      <SectionTitle> </SectionTitle>
      <Button title="Đăng xuất" tone="danger" onPress={confirmSignOut} />
    </Screen>
  );
}

const styles = StyleSheet.create({
  actions: { flexDirection: "row", gap: spacing.sm, marginTop: spacing.xs },
  dim: { color: colors.textDim, fontSize: 13 },
});

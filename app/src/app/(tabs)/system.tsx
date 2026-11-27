import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert, StyleSheet, Text, View } from "react-native";
import { Badge, Button, Card, CardTitle, ErrorBox, Loading, Row, Screen, SectionTitle } from "../../components/ui";
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

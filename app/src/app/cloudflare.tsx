import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import type { Schemas } from "../lib/types";

export default function Cloudflare() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);

  const status = useQuery({ queryKey: ["cloudflare"], queryFn: () => api<Schemas["CloudflareStatus"]>("/api/v1/cloudflare") });
  const zones = useQuery({ queryKey: ["cf-zones"], queryFn: () => api<Schemas["CloudflareZone"][]>("/api/v1/cloudflare/zones") });
  const tunnel = useQuery({ queryKey: ["cf-tunnel"], queryFn: () => api<Schemas["TunnelStatus"]>("/api/v1/cloudflare/tunnel/status") });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["cloudflare"] });
    queryClient.invalidateQueries({ queryKey: ["cf-tunnel"] });
    queryClient.invalidateQueries({ queryKey: ["cf-zones"] });
  };
  const publish = useMutation({
    mutationFn: () => api("/api/v1/cloudflare/tunnel/publish", { method: "POST", body: "{}" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const updateTunnel = useMutation({
    mutationFn: () => api("/api/v1/cloudflare/tunnel/update", { method: "POST", body: "{}" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const refreshZones = useMutation({
    mutationFn: () => api("/api/v1/cloudflare/zones/refresh", { method: "POST", body: "{}" }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmPublish() {
    Alert.alert("Publish tunnel", "Đồng bộ ingress rules của tunnel theo domains hiện tại?", [
      { text: "Hủy", style: "cancel" },
      { text: "Publish", onPress: () => publish.mutate() },
    ]);
  }

  const data = status.data;

  return (
    <Screen
      refreshing={status.isRefetching}
      onRefresh={() => {
        status.refetch();
        zones.refetch();
        tunnel.refetch();
      }}
    >
      <Stack.Screen options={{ title: "Cloudflare" }} />
      <ErrorBox error={error ?? status.error} />
      {status.isLoading ? (
        <Loading />
      ) : data ? (
        <Card>
          <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
            <CardTitle>Tunnel</CardTitle>
            <Badge label={`${data.replicas ?? 0} replicas`} tone={(data.replicas ?? 0) > 0 ? "ok" : "warn"} />
          </View>
          {data.panel_domain ? <Row label="Panel domain" value={data.panel_domain} /> : null}
          <Row label="Kết nối" value={data.connections ?? "—"} dim />
          <Row label="Hosts đã publish" value={data.published_hosts?.length ?? 0} dim />
          <Row label="Access" value={data.access_configured ? `đã cấu hình${data.access_enforced ? " (enforced)" : ""}` : "chưa"} dim />
          {data.replica_error ? <Row label="Lỗi replica" value={data.replica_error} /> : null}
          <View style={{ flexDirection: "row", gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}>
            <Button title="Publish ingress" small tone="ghost" loading={publish.isPending} onPress={confirmPublish} />
            {tunnel.data?.latest_release ? (
              <Button title="Cập nhật cloudflared" small tone="ghost" loading={updateTunnel.isPending} onPress={() => updateTunnel.mutate()} />
            ) : null}
          </View>
        </Card>
      ) : null}

      {tunnel.data ? (
        <>
          <SectionTitle>Kết nối tunnel</SectionTitle>
          {(tunnel.data.connections ?? []).length === 0 ? (
            <Empty text="Không có kết nối nào." />
          ) : (
            tunnel.data.connections.map((conn, i) => (
              <Card key={i}>
                <Row label="Node" value={(conn as Record<string, unknown>).name as string ?? `#${i + 1}`} dim />
              </Card>
            ))
          )}
          {tunnel.data.check_error ? (
            <Card>
              <Row label="Lỗi kiểm tra" value={tunnel.data.check_error} />
            </Card>
          ) : null}
        </>
      ) : null}

      <SectionTitle>Zones ({(zones.data ?? []).length})</SectionTitle>
      <View style={{ flexDirection: "row", justifyContent: "flex-end" }}>
        <Button title="Làm mới" small tone="ghost" loading={refreshZones.isPending} onPress={() => refreshZones.mutate()} />
      </View>
      {(zones.data ?? []).map((zone) => (
        <Card key={(zone as { id?: string }).id ?? zone.name}>
          <Row label="Zone" value={zone.name} />
          <Row label="Trạng thái" value={(zone as { status?: string }).status ?? "—"} dim />
        </Card>
      ))}
    </Screen>
  );
}

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import type { DomainView, ServiceSummary } from "../lib/types";

export default function Domains() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [host, setHost] = useState("");
  const [serviceId, setServiceId] = useState("");
  const [targetPort, setTargetPort] = useState("");

  const domains = useQuery({ queryKey: ["domains"], queryFn: () => api<DomainView[]>("/api/v1/domains") });
  const services = useQuery({ queryKey: ["services"], queryFn: () => api<ServiceSummary[]>("/api/v1/services") });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["domains"] });
  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/domains", {
        method: "POST",
        body: JSON.stringify({
          host: host.trim(),
          service_id: serviceId.trim(),
          target_port: Number(targetPort) || 80,
          ssl_mode: "auto",
        }),
      }),
    onSuccess: () => {
      setCreating(false);
      setHost("");
      setServiceId("");
      setTargetPort("");
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/domains/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const patch = useMutation({
    mutationFn: ({ id, body }: { id: string; body: Record<string, unknown> }) =>
      api(`/api/v1/domains/${id}`, { method: "PATCH", body: JSON.stringify(body) }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmDelete(domain: DomainView) {
    Alert.alert("Xóa tên miền", `Gỡ "${domain.host}"?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(domain.id) },
    ]);
  }

  const serviceName = (id: string | null | undefined) =>
    id == null ? "—" : (services.data?.find((s) => s.id === id)?.name ?? id.slice(0, 8));

  return (
    <Screen refreshing={domains.isRefetching} onRefresh={() => domains.refetch()}>
      <Stack.Screen options={{ title: "Tên miền" }} />
      <ErrorBox error={error ?? domains.error} />
      {domains.isLoading ? (
        <Loading />
      ) : (domains.data ?? []).length === 0 ? (
        <Empty text="Chưa có tên miền nào." />
      ) : (
        (domains.data ?? []).map((domain) => (
          <Card key={domain.id ?? domain.host}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{domain.host}</CardTitle>
              <Badge label={domain.published ? "published" : "unpublished"} tone={domain.published ? "ok" : "dim"} />
            </View>
            <Row label="Dịch vụ" value={serviceName(domain.service_id)} />
            <Row label="Port" value={domain.target_port} dim />
            <Row label="SSL" value={domain.ssl_mode ?? "—"} dim />
            {domain.matched_certificate_name ? <Row label="Cert" value={domain.matched_certificate_name} dim /> : null}
            <Row label="HTTPS redirect" value={domain.https_redirect ? "bật" : "tắt"} dim />
            <Row label="Cache" value={domain.cache_enabled ? "bật" : "tắt"} dim />
            <View style={{ flexDirection: "row", gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}>
              {domain.id != null && (
              <>
              <Button
                title={domain.published ? "Gỡ publish" : "Publish"}
                small
                tone="ghost"
                onPress={() => patch.mutate({ id: domain.id, body: { published: !domain.published } })}
              />
              <Button
                title={domain.https_redirect ? "Tắt redirect" : "Bật redirect"}
                small
                tone="ghost"
                onPress={() => patch.mutate({ id: domain.id, body: { https_redirect: !domain.https_redirect } })}
              />
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(domain)} />
              </>
              )}
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Thêm tên miền</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Tên miền mới</CardTitle>
          <Input value={host} onChangeText={setHost} placeholder="app.example.com" keyboardType="url" />
          <Input value={serviceId} onChangeText={setServiceId} placeholder="Service ID" />
          <Input value={targetPort} onChangeText={setTargetPort} placeholder="Port đích (vd 3000)" keyboardType="numeric" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!host.trim() || !serviceId.trim()} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Tên miền mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { bytes, timeAgo, type ServerView } from "../lib/types";

export default function Servers() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [kind, setKind] = useState("builder");
  const [publicIp, setPublicIp] = useState("");

  const servers = useQuery({ queryKey: ["servers"], queryFn: () => api<ServerView[]>("/api/v1/servers") });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["servers"] });
  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/servers", {
        method: "POST",
        body: JSON.stringify({ name: name.trim(), kind, public_ip: publicIp.trim() || null }),
      }),
    onSuccess: () => {
      setCreating(false);
      setName("");
      setPublicIp("");
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/servers/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const probe = useMutation({
    mutationFn: (id: string) => api(`/api/v1/servers/${id}/probe`, { method: "POST", body: "{}" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const prune = useMutation({
    mutationFn: (id: string) => api(`/api/v1/servers/${id}/prune`, { method: "POST", body: "{}" }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmDelete(server: ServerView) {
    Alert.alert("Xóa server", `Gỡ "${server.name}" khỏi panel?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(server.id) },
    ]);
  }

  return (
    <Screen refreshing={servers.isRefetching} onRefresh={() => servers.refetch()}>
      <Stack.Screen options={{ title: "Servers" }} />
      <ErrorBox error={error ?? servers.error} />
      {servers.isLoading ? (
        <Loading />
      ) : (servers.data ?? []).length === 0 ? (
        <Empty text="Chưa có server nào." />
      ) : (
        (servers.data ?? []).map((server) => (
          <Card key={server.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{server.name}</CardTitle>
              <Badge label={server.status} tone={server.status === "online" ? "ok" : server.status === "offline" ? "danger" : "dim"} />
            </View>
            <Row label="Loại" value={server.kind} dim />
            {server.hostname ? <Row label="Hostname" value={server.hostname} dim /> : null}
            {server.public_ip ? <Row label="IP" value={server.public_ip} dim /> : null}
            {server.cpu_count ? <Row label="CPU" value={`${server.cpu_count} cores`} /> : null}
            {server.memory_bytes ? <Row label="RAM" value={bytes(server.memory_bytes)} /> : null}
            {server.docker_version ? <Row label="Docker" value={server.docker_version} dim /> : null}
            {server.last_heartbeat_at ? <Row label="Heartbeat" value={timeAgo(server.last_heartbeat_at)} dim /> : null}
            {server.last_pruned_at ? <Row label="Prune gần nhất" value={timeAgo(server.last_pruned_at)} dim /> : null}
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end", flexWrap: "wrap" }}>
              <Button title="Probe" small tone="ghost" loading={probe.isPending} onPress={() => probe.mutate(server.id)} />
              <Button title="Prune" small tone="ghost" loading={prune.isPending} onPress={() => prune.mutate(server.id)} />
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(server)} />
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Thêm server</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Server mới</CardTitle>
          <Input value={name} onChangeText={setName} placeholder="Tên server" />
          <Input value={kind} onChangeText={setKind} placeholder="Loại (builder / agent)" />
          <Input value={publicIp} onChangeText={setPublicIp} placeholder="IP công khai (tuỳ chọn)" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!name.trim()} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Server mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

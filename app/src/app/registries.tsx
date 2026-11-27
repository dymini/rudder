import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import type { Schemas } from "../lib/types";

type RegistryView = Schemas["RegistryView"];

export default function Registries() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");

  const registries = useQuery({ queryKey: ["registries"], queryFn: () => api<RegistryView[]>("/api/v1/registries") });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["registries"] });
  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/registries", {
        method: "POST",
        body: JSON.stringify({
          name: name.trim(),
          url: url.trim(),
          username: username.trim() || null,
          password: password || null,
        }),
      }),
    onSuccess: () => {
      setCreating(false);
      setName("");
      setUrl("");
      setUsername("");
      setPassword("");
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/registries/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmDelete(registry: RegistryView) {
    Alert.alert("Xóa registry", `Xóa "${registry.name}"?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(registry.id) },
    ]);
  }

  return (
    <Screen refreshing={registries.isRefetching} onRefresh={() => registries.refetch()}>
      <Stack.Screen options={{ title: "Docker registries" }} />
      <ErrorBox error={error ?? registries.error} />
      {registries.isLoading ? (
        <Loading />
      ) : (registries.data ?? []).length === 0 ? (
        <Empty text="Chưa có registry nào." />
      ) : (
        (registries.data ?? []).map((registry) => (
          <Card key={registry.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{registry.name}</CardTitle>
              <Badge label={registry.has_password ? "có auth" : "không auth"} tone={registry.has_password ? "ok" : "dim"} />
            </View>
            <Row label="URL" value={registry.url} dim />
            {registry.username ? <Row label="Username" value={registry.username} dim /> : null}
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(registry)} />
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Thêm registry</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Registry mới</CardTitle>
          <Input value={name} onChangeText={setName} placeholder="Tên (vd Docker Hub)" />
          <Input value={url} onChangeText={setUrl} placeholder="URL (vd https://index.docker.io/v1)" keyboardType="url" />
          <Input value={username} onChangeText={setUsername} placeholder="Username (tuỳ chọn)" />
          <Input value={password} onChangeText={setPassword} placeholder="Password / token (tuỳ chọn)" secure />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!name.trim() || !url.trim()} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Registry mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

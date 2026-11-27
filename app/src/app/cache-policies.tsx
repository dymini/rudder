import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, Text, View } from "react-native";
import { Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors } from "../lib/theme";
import { bytes, type Schemas } from "../lib/types";

type CachePolicyView = Schemas["CachePolicyView"];

export default function CachePolicies() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  const policies = useQuery({ queryKey: ["cache-policies"], queryFn: () => api<CachePolicyView[]>("/api/v1/cache-policies") });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["cache-policies"] });
  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/cache-policies", {
        method: "POST",
        body: JSON.stringify({ name: name.trim(), description: description.trim(), rules: [] }),
      }),
    onSuccess: () => {
      setCreating(false);
      setName("");
      setDescription("");
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/cache-policies/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmDelete(policy: CachePolicyView) {
    Alert.alert("Xóa policy", `Xóa "${policy.name}"?${policy.domains > 0 ? ` ${policy.domains} tên miền đang dùng sẽ mất cache.` : ""}`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(policy.id) },
    ]);
  }

  return (
    <Screen refreshing={policies.isRefetching} onRefresh={() => policies.refetch()}>
      <Stack.Screen options={{ title: "Cache edge" }} />
      <ErrorBox error={error ?? policies.error} />
      {policies.isLoading ? (
        <Loading />
      ) : (policies.data ?? []).length === 0 ? (
        <Empty text="Chưa có cache policy nào." />
      ) : (
        (policies.data ?? []).map((policy) => (
          <Card key={policy.id}>
            <CardTitle>{policy.name}</CardTitle>
            {policy.description ? <Row label="Mô tả" value={policy.description} dim /> : null}
            <Row label="Tên miền dùng" value={policy.domains} />
            <Row label="RAM / Disk" value={`${bytes(policy.memory_max_bytes)} / ${bytes(policy.disk_max_bytes)}`} dim />
            <Row label="Rules" value={policy.rules.length} dim />
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
              {policy.rules.length > 0 ? (
                <Button title="Xem rules" small tone="ghost" onPress={() => setExpanded(expanded === policy.id ? null : policy.id)} />
              ) : null}
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(policy)} />
            </View>
            {expanded === policy.id
              ? policy.rules.map((rule, i) => (
                  <Text key={i} style={{ color: colors.textDim, fontSize: 12 }}>
                    {i + 1}. {rule.name ?? "rule"} — {rule.action?.mode ?? "cache"} ttl {rule.action?.ttl_secs ?? 0}s
                  </Text>
                ))
              : null}
          </Card>
        ))
      )}

      <SectionTitle>Tạo policy</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Policy mới</CardTitle>
          <Input value={name} onChangeText={setName} placeholder="Tên policy" />
          <Input value={description} onChangeText={setDescription} placeholder="Mô tả" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!name.trim()} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Policy mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

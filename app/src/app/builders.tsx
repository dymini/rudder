import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, Text, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors } from "../lib/theme";
import { bytes, timeAgo, type Schemas } from "../lib/types";

type BuilderView = Schemas["BuilderView"];
type BuilderJob = Schemas["BuilderJob"];

export default function Builders() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");

  const builders = useQuery({ queryKey: ["builders"], queryFn: () => api<BuilderView[]>("/api/v1/builders") });
  const jobs = useQuery({
    queryKey: ["builder-jobs", expanded],
    queryFn: () => api<BuilderJob[]>(`/api/v1/builders/${expanded}/jobs`),
    enabled: !!expanded,
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["builders"] });
  const create = useMutation({
    mutationFn: () => api("/api/v1/builders", { method: "POST", body: JSON.stringify({ name: name.trim() }) }),
    onSuccess: () => {
      setCreating(false);
      setName("");
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/builders/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const pruneCache = useMutation({
    mutationFn: (id: string) => api(`/api/v1/builders/${id}/cache/prune`, { method: "POST", body: "{}" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const patch = useMutation({
    mutationFn: ({ id, body }: { id: string; body: Record<string, unknown> }) =>
      api(`/api/v1/builders/${id}`, { method: "PATCH", body: JSON.stringify(body) }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmDelete(builder: BuilderView) {
    Alert.alert("Xóa builder", `Gỡ "${builder.name}"?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(builder.id) },
    ]);
  }

  return (
    <Screen refreshing={builders.isRefetching} onRefresh={() => builders.refetch()}>
      <Stack.Screen options={{ title: "Builders" }} />
      <ErrorBox error={error ?? builders.error} />
      {builders.isLoading ? (
        <Loading />
      ) : (builders.data ?? []).length === 0 ? (
        <Empty text="Chưa có builder nào." />
      ) : (
        (builders.data ?? []).map((builder) => (
          <Card key={builder.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{builder.name}</CardTitle>
              <Badge label={builder.online ? "online" : "offline"} tone={builder.online ? "ok" : "danger"} />
            </View>
            <Row label="Jobs đang chạy" value={`${builder.active_jobs}/${builder.max_jobs}`} />
            <Row label="Jobs xếp hàng" value={builder.queued_jobs} />
            <Row label="Cache" value={`${bytes(builder.cache_bytes)} / ${bytes(builder.cache_limit_bytes)}`} />
            {builder.arch ? <Row label="Arch" value={builder.arch} dim /> : null}
            {builder.version ? <Row label="Phiên bản" value={builder.version} dim /> : null}
            {builder.last_seen ? <Row label="Thấy lúc" value={timeAgo(builder.last_seen)} dim /> : null}
            <View style={{ flexDirection: "row", gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}>
              <Button
                title={builder.paused ? "Tiếp tục" : "Tạm dừng"}
                small
                tone="ghost"
                onPress={() => patch.mutate({ id: builder.id, body: { paused: !builder.paused } })}
              />
              <Button
                title={builder.enabled ? "Tắt" : "Bật"}
                small
                tone="ghost"
                onPress={() => patch.mutate({ id: builder.id, body: { enabled: !builder.enabled } })}
              />
              <Button title="Prune cache" small tone="ghost" loading={pruneCache.isPending} onPress={() => pruneCache.mutate(builder.id)} />
              <Button title="Jobs" small tone="ghost" onPress={() => setExpanded(expanded === builder.id ? null : builder.id)} />
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(builder)} />
            </View>
            {expanded === builder.id ? (
              jobs.isLoading ? (
                <Loading />
              ) : (jobs.data ?? []).length === 0 ? (
                <Text style={{ color: colors.textDim, fontSize: 13 }}>Không có job nào.</Text>
              ) : (
                (jobs.data ?? []).slice(0, 20).map((job) => (
                  <Row
                    key={job.id}
                    label={`${job.service_name} · ${job.status}`}
                    value={timeAgo(job.started_at ?? job.created_at)}
                    dim
                  />
                ))
              )
            ) : null}
          </Card>
        ))
      )}

      <SectionTitle>Thêm builder</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Builder mới</CardTitle>
          <Input value={name} onChangeText={setName} placeholder="Tên builder" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!name.trim()} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Builder mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

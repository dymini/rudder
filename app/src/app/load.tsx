import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { bytes, timeAgo, type Schemas } from "../lib/types";

type LoadView = Schemas["LoadView"];

export default function Load() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [editing, setEditing] = useState(false);
  const [cpu, setCpu] = useState("");
  const [mem, setMem] = useState("");

  const load = useQuery({ queryKey: ["load"], queryFn: () => api<LoadView>("/api/v1/load"), refetchInterval: 10000 });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["load"] });
  const pause = useMutation({
    mutationFn: (paused: boolean) => api("/api/v1/load/pause", { method: "POST", body: JSON.stringify({ paused }) }),
    onSuccess: invalidate,
    onError: setError,
  });
  const cancelBuilds = useMutation({
    mutationFn: () => api("/api/v1/load/cancel-builds", { method: "POST", body: "{}" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const saveSettings = useMutation({
    mutationFn: () =>
      api("/api/v1/load/settings", {
        method: "PUT",
        body: JSON.stringify({
          enabled: guard?.settings.enabled ?? true,
          cpu_percent: Number(cpu),
          memory_reserve_percent: Number(mem),
          build_concurrency: load.data?.builds.concurrency ?? 1,
        }),
      }),
    onSuccess: () => {
      setEditing(false);
      invalidate();
    },
    onError: setError,
  });

  function confirmCancel() {
    Alert.alert("Huỷ builds", "Huỷ toàn bộ builds đang chạy và xếp hàng?", [
      { text: "Hủy", style: "cancel" },
      { text: "Huỷ builds", style: "destructive", onPress: () => cancelBuilds.mutate() },
    ]);
  }

  const guard = load.data?.guard;
  const pressure = guard?.pressure;

  return (
    <Screen refreshing={load.isRefetching} onRefresh={() => load.refetch()}>
      <Stack.Screen options={{ title: "Tải hệ thống" }} />
      <ErrorBox error={error ?? load.error} />
      {load.isLoading ? (
        <Loading />
      ) : load.data ? (
        <>
          <Card>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>Load guard</CardTitle>
              <Badge label={guard?.holding ? "đang chặn build" : "bình thường"} tone={guard?.holding ? "warn" : "ok"} />
            </View>
            {pressure ? (
              <>
                <Row label="CPU" value={`${pressure.cpu_percent?.toFixed(0)}% (${pressure.cpu_count} cores)`} />
                <Row label="Load" value={`${pressure.load1?.toFixed(1)} / ${pressure.load5?.toFixed(1)} / ${pressure.load15?.toFixed(1)}`} dim />
                <Row label="RAM khả dụng" value={bytes(pressure.memory_available_bytes)} />
              </>
            ) : null}
            {guard?.error ? <Row label="Lỗi" value={guard.error} /> : null}
            {(guard?.reasons ?? []).map((r) => (
              <Row key={r.code} label={r.code} value={`${r.value} / ${r.limit}`} />
            ))}
            <Row label="Ngưỡng CPU" value={`${guard?.settings.cpu_percent}%`} dim />
            <Row label="Dự trữ RAM" value={`${guard?.settings.memory_reserve_percent}%`} dim />
            <Row label="Builds đã dừng" value={guard?.stopped_builds ?? 0} dim />
            <View style={{ flexDirection: "row", gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}>
              <Button
                title={guard?.settings.builds_paused ? "Tiếp tục builds" : "Tạm dừng builds"}
                small
                tone={guard?.settings.builds_paused ? "accent" : "ghost"}
                loading={pause.isPending}
                onPress={() => pause.mutate(!guard?.settings.builds_paused)}
              />
              <Button title="Huỷ builds" small tone="danger" onPress={confirmCancel} />
              <Button
                title="Ngưỡng"
                small
                tone="ghost"
                onPress={() => {
                  setCpu(String(guard?.settings.cpu_percent ?? ""));
                  setMem(String(guard?.settings.memory_reserve_percent ?? ""));
                  setEditing(!editing);
                }}
              />
            </View>
            {editing ? (
              <>
                <Input value={cpu} onChangeText={setCpu} placeholder="Ngưỡng CPU %" keyboardType="numeric" />
                <Input value={mem} onChangeText={setMem} placeholder="Dự trữ RAM %" keyboardType="numeric" />
                <Button title="Lưu ngưỡng" small loading={saveSettings.isPending} onPress={() => saveSettings.mutate()} />
              </>
            ) : null}
          </Card>

          <SectionTitle>Builds</SectionTitle>
          <Card>
            <Row label="Đang chạy" value={load.data.builds.running} />
            <Row label="Xếp hàng" value={load.data.builds.queued} />
            <Row label="Giới hạn đồng thời" value={load.data.builds.concurrency} dim />
            <Row label="Stack actions" value={load.data.builds.stack_actions} dim />
            {load.data.builds.oldest_queued_at ? (
              <Row label="Job cũ nhất" value={timeAgo(load.data.builds.oldest_queued_at)} dim />
            ) : null}
          </Card>

          <SectionTitle>Bảo vệ tiến trình</SectionTitle>
          {(load.data.protection ?? []).map((p) => (
            <Card key={p.name}>
              <Row label="Tên" value={p.name} />
              <Row label="Vai trò" value={p.role} dim />
              <Row
                label="Trạng thái"
                value={<Badge label={p.present ? (p.protected ? "được bảo vệ" : "chưa bảo vệ") : "vắng"} tone={p.protected ? "ok" : p.present ? "warn" : "dim"} />}
              />
            </Card>
          ))}
        </>
      ) : null}
    </Screen>
  );
}

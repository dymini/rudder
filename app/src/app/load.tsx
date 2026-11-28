import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, StyleSheet, Text, View } from "react-native";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  ErrorBox,
  GroupedRow,
  HairlineBar,
  Input,
  InsetGroup,
  Loading,
  MetricLedger,
  Row,
  Screen,
  SectionTitle,
} from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors, monoFont, spacing } from "../lib/theme";
import { bytes, timeAgo, type Schemas } from "../lib/types";

type LoadView = Schemas["LoadView"];

export default function Load() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [editing, setEditing] = useState(false);
  const [cpu, setCpu] = useState("");
  const [mem, setMem] = useState("");

  const load = useQuery({
    queryKey: ["load"],
    queryFn: () => api<LoadView>("/api/v1/load"),
    refetchInterval: 10000,
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["load"] });
  const pause = useMutation({
    mutationFn: (paused: boolean) =>
      api("/api/v1/load/pause", { method: "POST", body: JSON.stringify({ paused }) }),
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
  const memUsedPct =
    pressure && pressure.memory_total_bytes > 0
      ? 100 - (pressure.memory_available_bytes / pressure.memory_total_bytes) * 100
      : 0;

  return (
    <Screen refreshing={load.isRefetching} onRefresh={() => load.refetch()}>
      <Stack.Screen options={{ title: "Tải hệ thống" }} />
      <ErrorBox error={error ?? load.error} />

      {load.isLoading ? (
        <Loading />
      ) : load.data ? (
        <>
          <Card>
            <View style={styles.headerRow}>
              <CardTitle>Load guard</CardTitle>
              <Badge
                label={guard?.holding ? "đang chặn build" : "bình thường"}
                tone={guard?.holding ? "warn" : "ok"}
              />
            </View>

            {pressure ? (
              <View style={styles.gauges}>
                <View style={styles.gaugeBlock}>
                  <View style={styles.gaugeHead}>
                    <Text style={styles.gaugeLabel}>
                      CPU ({pressure.cpu_count} cores · load {pressure.load1?.toFixed(2)})
                    </Text>
                    <Text style={styles.gaugeVal}>{Math.round(pressure.cpu_percent ?? 0)}%</Text>
                  </View>
                  <HairlineBar
                    value={pressure.cpu_percent ?? 0}
                    warn={guard?.settings.cpu_percent ?? 90}
                    danger={96}
                  />
                </View>

                <View style={styles.gaugeBlock}>
                  <View style={styles.gaugeHead}>
                    <Text style={styles.gaugeLabel}>
                      RAM (trống {bytes(pressure.memory_available_bytes)})
                    </Text>
                    <Text style={styles.gaugeVal}>{Math.round(memUsedPct)}%</Text>
                  </View>
                  <HairlineBar
                    value={memUsedPct}
                    warn={100 - (guard?.settings.memory_reserve_percent ?? 15)}
                    danger={93}
                  />
                </View>
              </View>
            ) : null}

            {guard?.error ? <Row label="Lỗi" value={guard.error} /> : null}
            {(guard?.reasons ?? []).map((r) => (
              <Row key={r.code} label={r.code} value={`${r.value} / ${r.limit}`} mono />
            ))}
            <Row label="Ngưỡng CPU / Dự trữ RAM" value={`${guard?.settings.cpu_percent}% · ${guard?.settings.memory_reserve_percent}%`} dim mono />

            <View style={styles.actions}>
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
              <View style={{ gap: spacing.sm, marginTop: spacing.xs }}>
                <Input value={cpu} onChangeText={setCpu} placeholder="Ngưỡng CPU %" keyboardType="numeric" />
                <Input value={mem} onChangeText={setMem} placeholder="Dự trữ RAM %" keyboardType="numeric" />
                <Button title="Lưu ngưỡng" small loading={saveSettings.isPending} onPress={() => saveSettings.mutate()} />
              </View>
            ) : null}
          </Card>

          <SectionTitle>Hàng đợi build</SectionTitle>
          <MetricLedger
            items={[
              { label: "Đang chạy", value: String(load.data.builds.running), suffix: `/ ${load.data.builds.concurrency}` },
              { label: "Xếp hàng", value: String(load.data.builds.queued), tone: load.data.builds.queued > 0 ? "warn" : "dim" },
              {
                label: "Đã dừng",
                value: String(guard?.stopped_builds ?? 0),
                caption: load.data.builds.oldest_queued_at ? timeAgo(load.data.builds.oldest_queued_at) : undefined,
              },
            ]}
          />

          <SectionTitle>Bảo vệ tiến trình</SectionTitle>
          <InsetGroup>
            {(load.data.protection ?? []).map((p) => (
              <GroupedRow
                key={p.name}
                title={p.name}
                subtitle={p.role}
                right={
                  <Badge
                    label={p.present ? (p.protected ? "được bảo vệ" : "chưa bảo vệ") : "vắng"}
                    tone={p.protected ? "ok" : p.present ? "warn" : "dim"}
                  />
                }
              />
            ))}
          </InsetGroup>
        </>
      ) : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  headerRow: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
  },
  gauges: {
    gap: 12,
    marginVertical: 4,
  },
  gaugeBlock: {
    gap: 6,
  },
  gaugeHead: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "baseline",
  },
  gaugeLabel: {
    color: colors.textDim,
    fontSize: 12,
  },
  gaugeVal: {
    color: colors.text,
    fontFamily: monoFont,
    fontSize: 12.5,
    fontWeight: "600",
  },
  actions: {
    flexDirection: "row",
    gap: 8,
    flexWrap: "wrap",
    marginTop: 4,
  },
});

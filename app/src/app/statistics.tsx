import { useQuery } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { StyleSheet, Text, View } from "react-native";
import {
  Card,
  CardTitle,
  Empty,
  ErrorBox,
  HairlineBar,
  Loading,
  MetricLedger,
  Row,
  Screen,
  SectionTitle,
  SegmentedControl,
} from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors, monoFont, spacing } from "../lib/theme";
import { bytes, timeAgo, type Schemas } from "../lib/types";

type StatsOverview = Schemas["StatsOverview"];
type StatsRange = "1h" | "24h" | "7d" | "30d";

const RANGES: readonly { value: StatsRange; label: string }[] = [
  { value: "1h", label: "1h" },
  { value: "24h", label: "24h" },
  { value: "7d", label: "7d" },
  { value: "30d", label: "30d" },
];

function TopBarCard({
  title,
  items,
  color,
}: {
  title: string;
  items: { id: string; name: string; raw: number; formatted: string }[];
  color?: string;
}) {
  const max = Math.max(...items.map((i) => i.raw), 1);
  return (
    <Card>
      <CardTitle>{title}</CardTitle>
      {items.length === 0 ? (
        <Empty text="Chưa có số liệu." />
      ) : (
        <View style={{ gap: 10 }}>
          {items.map((item) => (
            <View key={item.id} style={{ gap: 4 }}>
              <View style={styles.barHead}>
                <Text style={styles.barName} numberOfLines={1}>
                  {item.name}
                </Text>
                <Text style={styles.barVal}>{item.formatted}</Text>
              </View>
              <HairlineBar value={(item.raw / max) * 100} color={color} />
            </View>
          ))}
        </View>
      )}
    </Card>
  );
}

export default function Statistics() {
  const { api } = useAuth();
  const [range, setRange] = useState<StatsRange>("24h");

  const stats = useQuery({
    queryKey: ["stats", range],
    queryFn: () => api<StatsOverview>(`/api/v1/stats/overview?range=${range}`),
    refetchInterval: 30000,
  });

  const totals = stats.data?.metrics.totals;
  const ops = stats.data?.operations;

  return (
    <Screen refreshing={stats.isRefetching} onRefresh={() => stats.refetch()}>
      <Stack.Screen options={{ title: "Thống kê" }} />

      <SegmentedControl<StatsRange> value={range} onChange={setRange} options={RANGES} />

      <ErrorBox error={stats.error} />

      {stats.isLoading ? (
        <Loading />
      ) : stats.data ? (
        <>
          <SectionTitle>Lưu lượng & lỗi</SectionTitle>
          <MetricLedger
            items={[
              {
                label: "Requests",
                value: totals?.requests != null ? Math.round(totals.requests).toLocaleString() : "—",
                caption: `↓${bytes(totals?.rx_bytes)}`,
              },
              {
                label: "Lỗi 5xx",
                value: String(totals?.http_5xx ?? 0),
                caption: `${((totals?.http_5xx_rate ?? 0) * 100).toFixed(2)}%`,
                tone: (totals?.http_5xx ?? 0) > 0 ? "danger" : "ok",
              },
              {
                label: "Băng thông ra",
                value: bytes(totals?.tx_bytes),
                caption: `${stats.data.metrics.container_restarts} restart`,
              },
            ]}
          />

          <SectionTitle>Tài nguyên cụm máy chủ</SectionTitle>
          <MetricLedger
            items={[
              {
                label: "CPU TB / Đỉnh",
                value: `${totals?.cpu_avg?.toFixed(0) ?? "—"}%`,
                suffix: `/ ${totals?.cpu_peak?.toFixed(0) ?? "—"}%`,
                tone: (totals?.cpu_peak ?? 0) >= 90 ? "warn" : "dim",
              },
              {
                label: "RAM TB / Đỉnh",
                value: `${totals?.memory_avg?.toFixed(0) ?? "—"}%`,
                suffix: `/ ${totals?.memory_peak?.toFixed(0) ?? "—"}%`,
                tone: (totals?.memory_peak ?? 0) >= 90 ? "warn" : "dim",
              },
            ]}
          />

          <SectionTitle>Top dịch vụ</SectionTitle>
          <TopBarCard
            title="Theo requests"
            items={(stats.data.metrics.top_requests ?? []).slice(0, 5).map((s) => ({
              id: s.service_id,
              name: s.name,
              raw: s.requests ?? 0,
              formatted: Math.round(s.requests ?? 0).toLocaleString(),
            }))}
          />
          <TopBarCard
            title="Theo CPU"
            items={(stats.data.metrics.top_cpu ?? []).slice(0, 5).map((s) => ({
              id: s.service_id,
              name: s.name,
              raw: s.cpu_percent ?? 0,
              formatted: `${(s.cpu_percent ?? 0).toFixed(1)}%`,
            }))}
          />
          <TopBarCard
            title="Theo RAM"
            items={(stats.data.metrics.top_memory ?? []).slice(0, 5).map((s) => ({
              id: s.service_id,
              name: s.name,
              raw: s.memory_bytes ?? 0,
              formatted: bytes(s.memory_bytes),
            }))}
            color={colors.busy}
          />
          {(stats.data.metrics.top_5xx ?? []).length > 0 ? (
            <TopBarCard
              title="Theo lỗi 5xx"
              items={(stats.data.metrics.top_5xx ?? []).slice(0, 5).map((s) => ({
                id: s.service_id,
                name: s.name,
                raw: s.http_5xx ?? 0,
                formatted: String(s.http_5xx ?? 0),
              }))}
              color={colors.danger}
            />
          ) : null}

          {ops ? (
            <>
              <SectionTitle>Vận hành</SectionTitle>
              <Card>
                <Row
                  label="Deploys"
                  value={`${ops.deploys} (${((ops.deploy_success_rate ?? 0) * 100).toFixed(0)}% thành công)`}
                />
                <Row
                  label="Thời gian deploy TB"
                  value={
                    ops.deploy_avg_duration_seconds
                      ? `${Math.round(ops.deploy_avg_duration_seconds)}s`
                      : "—"
                  }
                  dim
                  mono
                />
                <Row label="Backup runs" value={`${ops.backup_runs} (${ops.backup_failures} lỗi)`} />
                <Row label="Alerts đã kích" value={ops.alerts_fired} dim />
                <Row label="MCP calls" value={`${ops.mcp_calls} (${ops.mcp_denied} bị chặn)`} dim />
                <Row
                  label="Quy mô"
                  value={`${ops.counts.services} dịch vụ · ${ops.counts.servers} máy chủ · ${ops.counts.domains} tên miền`}
                  dim
                />
              </Card>

              <SectionTitle>Sao lưu panel</SectionTitle>
              <Card>
                <Row label="Đã cấu hình" value={stats.data.panel_backup?.configured ? "Có" : "Chưa"} dim />
                {stats.data.panel_backup?.last_success ? (
                  <Row label="Thành công lúc" value={timeAgo(stats.data.panel_backup.last_success)} mono />
                ) : null}
                {stats.data.panel_backup?.warning ? (
                  <Row label="Cảnh báo" value={stats.data.panel_backup.warning} />
                ) : null}
              </Card>
            </>
          ) : null}
        </>
      ) : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  barHead: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "baseline",
    gap: spacing.sm,
  },
  barName: {
    color: colors.text,
    fontSize: 13.5,
    fontWeight: "500",
    flex: 1,
  },
  barVal: {
    color: colors.textSoft,
    fontFamily: monoFont,
    fontSize: 12,
    fontWeight: "600",
  },
});

import { useQuery } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Button, Card, CardTitle, ErrorBox, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { bytes, timeAgo, type Schemas } from "../lib/types";

type StatsOverview = Schemas["StatsOverview"];

const RANGES = ["1h", "24h", "7d", "30d"];

export default function Statistics() {
  const { api } = useAuth();
  const [range, setRange] = useState("24h");

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
      <Row
        label="Khoảng thời gian"
        value={
          <>
            {RANGES.map((r) => (
              <Button key={r} title={r} small tone={range === r ? "accent" : "ghost"} onPress={() => setRange(r)} />
            ))}
          </>
        }
      />
      <ErrorBox error={stats.error} />
      {stats.isLoading ? (
        <Loading />
      ) : stats.data ? (
        <>
          <Card>
            <CardTitle>Lưu lượng</CardTitle>
            <Row label="Requests" value={totals?.requests ?? 0} />
            <Row label="Lỗi 5xx" value={`${totals?.http_5xx ?? 0} (${((totals?.http_5xx_rate ?? 0) * 100).toFixed(2)}%)`} />
            <Row label="CPU trung bình / đỉnh" value={`${totals?.cpu_avg?.toFixed(0) ?? "—"}% / ${totals?.cpu_peak?.toFixed(0) ?? "—"}%`} />
            <Row label="RAM trung bình / đỉnh" value={`${totals?.memory_avg?.toFixed(0) ?? "—"}% / ${totals?.memory_peak?.toFixed(0) ?? "—"}%`} />
            <Row label="Băng thông" value={`↓${bytes(totals?.rx_bytes)} ↑${bytes(totals?.tx_bytes)}`} dim />
            <Row label="Container restart" value={stats.data.metrics.container_restarts} dim />
          </Card>

          <SectionTitle>Top dịch vụ</SectionTitle>
          <Card>
            <CardTitle>Theo requests</CardTitle>
            {(stats.data.metrics.top_requests ?? []).slice(0, 5).map((s) => (
              <Row key={s.service_id} label={s.name} value={s.requests ?? 0} dim />
            ))}
          </Card>
          <Card>
            <CardTitle>Theo CPU</CardTitle>
            {(stats.data.metrics.top_cpu ?? []).slice(0, 5).map((s) => (
              <Row key={s.service_id} label={s.name} value={`${s.cpu_percent?.toFixed(1)}%`} dim />
            ))}
          </Card>
          <Card>
            <CardTitle>Theo RAM</CardTitle>
            {(stats.data.metrics.top_memory ?? []).slice(0, 5).map((s) => (
              <Row key={s.service_id} label={s.name} value={bytes(s.memory_bytes)} dim />
            ))}
          </Card>
          <Card>
            <CardTitle>Theo lỗi 5xx</CardTitle>
            {(stats.data.metrics.top_5xx ?? []).slice(0, 5).map((s) => (
              <Row key={s.service_id} label={s.name} value={s.http_5xx ?? 0} dim />
            ))}
          </Card>

          {ops ? (
            <>
              <SectionTitle>Vận hành</SectionTitle>
              <Card>
                <Row label="Deploys" value={`${ops.deploys} (thành công ${((ops.deploy_success_rate ?? 0) * 100).toFixed(0)}%)`} />
                <Row label="Thời gian deploy TB" value={ops.deploy_avg_duration_seconds ? `${Math.round(ops.deploy_avg_duration_seconds)}s` : "—"} dim />
                <Row label="Backup runs" value={`${ops.backup_runs} (${ops.backup_failures} lỗi)`} />
                <Row label="Alerts đã kích" value={ops.alerts_fired} dim />
                <Row label="MCP calls" value={`${ops.mcp_calls} (${ops.mcp_denied} bị chặn)`} dim />
                <Row label="Quy mô" value={`${ops.counts.services} dịch vụ · ${ops.counts.servers} servers · ${ops.counts.domains} domains`} dim />
              </Card>
              <SectionTitle>Sao lưu panel</SectionTitle>
              <Card>
                <Row label="Đã cấu hình" value={stats.data.panel_backup?.configured ? "có" : "chưa"} dim />
                {stats.data.panel_backup?.last_success ? (
                  <Row label="Thành công lúc" value={timeAgo(stats.data.panel_backup.last_success)} />
                ) : null}
                {stats.data.panel_backup?.warning ? <Row label="Cảnh báo" value={stats.data.panel_backup.warning} /> : null}
              </Card>
            </>
          ) : null}
        </>
      ) : null}
    </Screen>
  );
}

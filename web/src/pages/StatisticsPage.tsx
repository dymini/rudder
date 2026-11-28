import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { Link } from "react-router-dom";
import {
  Area,
  AreaChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api } from "../api";
import { ThresholdMeter } from "../components/load";
import { AXIS_LINE, COLORS, TICK, TOOLTIP_STYLES } from "../components/metrics";
import { KpiCard, LiveControlBar, SectionHeading } from "../components/ops-ui";
import { PanelBackupStatus } from "../components/panel-backups";
import { Badge, Button, Card, Empty, Skeleton } from "../components/ui";
import { formatBytes, formatDuration, formatTime } from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";

type Top = Schemas["TopService"];
type Point = Schemas["StatsPoint"];
type Metric =
  | "network_bytes"
  | "requests"
  | "http_5xx"
  | "cpu_percent"
  | "memory_bytes";

const STATS_RANGES = ["1h", "24h", "7d", "30d"] as const;
type StatsRange = (typeof STATS_RANGES)[number];

const number = (value: number | null | undefined) =>
  value == null ? "—" : Math.round(value).toLocaleString();
const percent = (value: number | null | undefined) =>
  value == null ? "—" : `${value.toFixed(1)}%`;
const rate = (value: number | null | undefined) =>
  value == null ? "—" : percent(value * 100);

function StatsChartCard({
  title,
  series,
  range,
  lines,
  format,
  colors = COLORS,
}: {
  title: string;
  series: Point[];
  range: StatsRange;
  lines: [keyof Point, string][];
  format: (value: number | null | undefined) => string;
  colors?: string[];
}) {
  const t = useT();
  const baseId = useId();
  const hasData = series.some((point) => lines.some(([key]) => point[key] != null));

  return (
    <Card title={title}>
      {hasData ? (
        <ResponsiveContainer width="100%" height={230}>
          <AreaChart data={series} margin={{ top: 6, left: 0, right: 10, bottom: 0 }}>
            <defs>
              {lines.map(([key], i) => {
                const stroke = colors[i % colors.length];
                return (
                  <linearGradient key={key} id={`${baseId}-${i}`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor={stroke} stopOpacity={0.22} />
                    <stop offset="95%" stopColor={stroke} stopOpacity={0.01} />
                  </linearGradient>
                );
              })}
            </defs>
            <CartesianGrid stroke="var(--line)" vertical={false} strokeDasharray="3 3" />
            <XAxis
              dataKey="at"
              tick={TICK}
              axisLine={AXIS_LINE}
              tickLine={false}
              minTickGap={40}
              tickFormatter={(value: string) =>
                new Date(value).toLocaleString(
                  undefined,
                  range === "1h" || range === "24h"
                    ? { hour: "2-digit", minute: "2-digit" }
                    : { month: "short", day: "numeric", hour: "2-digit" },
                )
              }
            />
            <YAxis
              width={74}
              tick={TICK}
              axisLine={false}
              tickLine={false}
              tickFormatter={(value: number) => format(value)}
            />
            <Tooltip
              {...TOOLTIP_STYLES}
              labelFormatter={(value) => new Date(String(value)).toLocaleString()}
              formatter={(value, name) => [format(Number(value)), name]}
            />
            <Legend wrapperStyle={{ fontSize: 12, color: "var(--ink-soft)" }} />
            {lines.map(([key, name], i) => (
              <Area
                key={key}
                type="monotone"
                dataKey={key}
                name={name}
                stroke={colors[i % colors.length]}
                fill={`url(#${baseId}-${i})`}
                strokeWidth={1.85}
                isAnimationActive={false}
              />
            ))}
          </AreaChart>
        </ResponsiveContainer>
      ) : (
        <Empty>{t("No statistics available in this range.")}</Empty>
      )}
    </Card>
  );
}

function TopServicesCard({
  title,
  rows,
  metric,
  format,
  tone = "pine",
}: {
  title: string;
  rows: Top[];
  metric: Metric;
  format: (value: number | null | undefined) => string;
  tone?: "pine" | "danger";
}) {
  const t = useT();
  const max = Math.max(...rows.map((r) => Number(r[metric] ?? 0)), 1);

  return (
    <Card title={title}>
      {rows.length ? (
        <ol className="space-y-3">
          {rows.map((row, i) => {
            const raw = Number(row[metric] ?? 0);
            const pct = Math.min(100, Math.max(2, (raw / max) * 100));
            return (
              <li key={row.service_id} className="space-y-1 text-sm">
                <div className="flex min-w-0 items-baseline justify-between gap-3">
                  <Link
                    className="min-w-0 truncate font-medium text-ink hover:text-pine"
                    to={`/services/${row.service_id}`}
                  >
                    <span className="mr-1.5 text-xs text-faint tabular-nums">{i + 1}.</span>
                    {row.name}
                  </Link>
                  <span className="shrink-0 font-medium text-ink-soft tabular-nums">
                    {format(row[metric])}
                  </span>
                </div>
                <div className="h-1.5 overflow-hidden rounded-full bg-sunken">
                  <div
                    className={`h-full rounded-full ${tone === "danger" ? "bg-danger" : "bg-pine"}`}
                    style={{ width: `${pct}%` }}
                  />
                </div>
              </li>
            );
          })}
        </ol>
      ) : (
        <Empty>{t("No statistics available in this range.")}</Empty>
      )}
    </Card>
  );
}

export function StatisticsPage() {
  const t = useT();
  const queryClient = useQueryClient();
  const [range, setRange] = useState<StatsRange>("1h");
  const [paused, setPaused] = useState(false);

  const query = useQuery({
    queryKey: ["statistics", range],
    queryFn: () => api<Schemas["StatsOverview"]>(`/api/v1/stats/overview?range=${range}`),
    refetchInterval: paused ? false : 60_000,
  });

  const data = query.data;
  const metrics = data?.metrics;
  const totals = metrics?.totals;
  const ops = data?.operations;

  return (
    <div className="min-w-0 space-y-6">
      <header className="flex flex-wrap items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="text-xl leading-tight font-semibold tracking-[-0.015em] text-ink sm:text-2xl">
            {t("Statistics")}
          </h1>
          <p className="mt-0.5 text-sm text-muted">
            {t("Traffic, resources, and operations across Dymini Panel.")}
          </p>
        </div>
        <LiveControlBar
          ranges={STATS_RANGES}
          range={range}
          onRange={setRange}
          paused={paused}
          onTogglePause={() => setPaused((p) => !p)}
          onRefresh={() => void queryClient.invalidateQueries({ queryKey: ["statistics", range] })}
          refreshing={query.isFetching}
          updatedAt={query.dataUpdatedAt}
        />
      </header>

      {data ? (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-card border border-line bg-surface px-4 py-3 shadow-card sm:px-5">
          <div className="min-w-0 flex-1">
            <PanelBackupStatus health={data.panel_backup} />
          </div>
          <Link
            className="shrink-0 text-sm font-medium text-pine hover:underline"
            to="/system#panel-backups"
          >
            {t("Manage panel backups")}
          </Link>
        </div>
      ) : null}

      {query.isPending ? (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          {Array.from({ length: 4 }, (_, i) => (
            <Skeleton key={i} className="h-28 rounded-card" />
          ))}
        </div>
      ) : null}

      {query.isError ? (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-3 rounded-card border border-danger-line bg-danger-soft px-4 py-3 text-danger"
        >
          <span className="min-w-0 flex-1 break-words text-sm">{query.error.message}</span>
          <Button quiet size="sm" onClick={() => query.refetch()}>
            {t("Retry")}
          </Button>
        </div>
      ) : null}

      {data && metrics && ops && totals ? (
        <>
          <p className="break-words text-xs text-muted tabular-nums">
            {formatTime(data.since)} — {formatTime(data.until)} ·{" "}
            {t("Metric resolution: {resolution}", { resolution: data.resolution })} ·{" "}
            {t("Totals are estimates from retained samples.")}
          </p>

          {/* Group 1: Traffic & Edge */}
          <section className="space-y-3">
            <SectionHeading title={t("Traffic & edge")} />
            <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-5">
              <KpiCard icon="globe" label={t("Proxy requests")} value={number(totals.requests)} />
              <KpiCard
                icon="alert"
                label={t("Proxy 5xx")}
                value={number(totals.http_5xx)}
                tone={(totals.http_5xx ?? 0) > 0 ? "danger" : "default"}
              />
              <KpiCard
                icon="activity"
                label={t("Proxy 5xx rate")}
                value={rate(totals.http_5xx_rate)}
                tone={(totals.http_5xx_rate ?? 0) > 0.01 ? "danger" : "default"}
              />
              <KpiCard
                icon="download"
                label={t("Network received")}
                value={formatBytes(totals.rx_bytes)}
              />
              <KpiCard
                icon="arrowRight"
                label={t("Network sent")}
                value={formatBytes(totals.tx_bytes)}
              />
            </div>
          </section>

          {/* Group 2: Compute Fleet */}
          <section className="space-y-3">
            <SectionHeading title={t("Compute fleet")} />
            <div className="grid grid-cols-2 gap-3 sm:gap-4 lg:grid-cols-4">
              <KpiCard icon="cpu" label={t("Average server CPU")} value={percent(totals.cpu_avg)} />
              <KpiCard
                icon="cpu"
                label={t("Peak server CPU")}
                value={percent(totals.cpu_peak)}
                tone={(totals.cpu_peak ?? 0) >= 90 ? "warning" : "default"}
              />
              <KpiCard
                icon="memory"
                label={t("Average server memory")}
                value={percent(totals.memory_avg)}
              />
              <KpiCard
                icon="memory"
                label={t("Peak server memory")}
                value={percent(totals.memory_peak)}
                tone={(totals.memory_peak ?? 0) >= 90 ? "warning" : "default"}
              />
            </div>
          </section>

          {/* Group 3: Deployments, Operations & MCP */}
          <div className="grid gap-6 xl:grid-cols-2">
            <section className="space-y-3">
              <SectionHeading title={t("Deployments & reliability")} />
              <div className="grid grid-cols-2 gap-3 sm:gap-4">
                <KpiCard icon="rocket" label={t("Deploys in range")} value={number(ops.deploys)} />
                <KpiCard
                  icon="check"
                  label={t("Deploy success rate")}
                  value={rate(ops.deploy_success_rate)}
                  tone={
                    ops.deploy_success_rate != null && ops.deploy_success_rate < 0.9
                      ? "warning"
                      : "pine"
                  }
                />
                <KpiCard
                  icon="clock"
                  label={t("Average deploy duration")}
                  value={formatDuration(
                    ops.deploy_avg_duration_seconds == null
                      ? null
                      : ops.deploy_avg_duration_seconds * 1000,
                  )}
                />
                <KpiCard
                  icon="refresh"
                  label={t("Observed restart increases")}
                  value={number(metrics.container_restarts)}
                  tone={(metrics.container_restarts ?? 0) > 0 ? "warning" : "default"}
                />
              </div>
            </section>

            <section className="space-y-3">
              <SectionHeading title={t("Backups, alerts & AI agents")} />
              <div className="grid grid-cols-2 gap-3 sm:gap-4">
                <KpiCard
                  icon="archive"
                  label={t("Backup runs in range")}
                  value={number(ops.backup_runs)}
                  caption={
                    ops.backup_failures
                      ? t("{count} failures", { count: ops.backup_failures })
                      : t("No backup failures")
                  }
                  tone={ops.backup_failures > 0 ? "danger" : "default"}
                />
                <KpiCard
                  icon="bell"
                  label={t("Alerts fired in range")}
                  value={number(ops.alerts_fired)}
                  tone={ops.alerts_fired > 0 ? "warning" : "default"}
                  caption={t("{servers} servers · {services} services · {domains} domains", {
                    servers: ops.counts.servers,
                    services: ops.counts.services,
                    domains: ops.counts.domains,
                  })}
                />
                <KpiCard
                  icon="command"
                  label={t("MCP calls")}
                  value={number(ops.mcp_calls)}
                  caption={t("Error rate {rate}", { rate: rate(ops.mcp_error_rate) })}
                />
                <KpiCard
                  icon="shield"
                  label={t("Denied MCP calls")}
                  value={number(ops.mcp_denied)}
                  tone={ops.mcp_denied > 0 ? "warning" : "default"}
                />
              </div>
            </section>
          </div>

          {/* Time-series Charts */}
          <section className="space-y-3">
            <SectionHeading title={t("Trends over time")} />
            <div className="grid min-w-0 gap-4 xl:grid-cols-2">
              <StatsChartCard
                title={t("Proxy requests per bucket")}
                series={metrics.series}
                range={range}
                lines={[
                  ["requests", t("Proxy requests")],
                  ["http_5xx", t("Proxy 5xx")],
                ]}
                format={number}
                colors={["var(--chart-1)", "var(--chart-4)"]}
              />
              <StatsChartCard
                title={t("Network volume per bucket")}
                series={metrics.series}
                range={range}
                lines={[
                  ["rx_bytes", t("Network received")],
                  ["tx_bytes", t("Network sent")],
                ]}
                format={formatBytes}
                colors={["var(--chart-1)", "var(--chart-3)"]}
              />
              <StatsChartCard
                title={t("Server CPU over time")}
                series={metrics.series}
                range={range}
                lines={[
                  ["cpu_avg", t("Average server CPU")],
                  ["cpu_peak", t("Peak server CPU")],
                ]}
                format={percent}
              />
              <StatsChartCard
                title={t("Server memory over time")}
                series={metrics.series}
                range={range}
                lines={[
                  ["memory_avg", t("Average server memory")],
                  ["memory_peak", t("Peak server memory")],
                ]}
                format={percent}
              />
              <StatsChartCard
                title={t("Proxy 5xx rate")}
                series={metrics.series}
                range={range}
                lines={[["http_5xx_rate", t("Proxy 5xx rate")]]}
                format={rate}
                colors={["var(--chart-4)"]}
              />
            </div>
          </section>

          {/* Top Services Leaderboards */}
          <section className="space-y-3">
            <SectionHeading title={t("Top services")} />
            <div className="grid min-w-0 gap-4 md:grid-cols-2 xl:grid-cols-3">
              <TopServicesCard
                title={t("Top services by requests")}
                rows={metrics.top_requests}
                metric="requests"
                format={number}
              />
              <TopServicesCard
                title={t("Top services by 5xx")}
                rows={metrics.top_5xx}
                metric="http_5xx"
                format={number}
                tone="danger"
              />
              <TopServicesCard
                title={t("Top services by network volume")}
                rows={metrics.top_network}
                metric="network_bytes"
                format={formatBytes}
              />
              <TopServicesCard
                title={t("Top services by CPU")}
                rows={metrics.top_cpu}
                metric="cpu_percent"
                format={percent}
              />
              <TopServicesCard
                title={t("Top services by memory")}
                rows={metrics.top_memory}
                metric="memory_bytes"
                format={formatBytes}
              />
            </div>
          </section>

          {/* Disks & Backup Health */}
          <div className="grid min-w-0 gap-4 lg:grid-cols-2">
            <Card title={t("Latest disk usage by server")}>
              <ul className="space-y-4">
                {metrics.disks.map((disk) => (
                  <li key={disk.server_id} className="min-w-0 space-y-1.5 border-b border-line pb-3 last:border-b-0 last:pb-0">
                    <div className="flex items-baseline justify-between gap-2 text-sm">
                      <span className="truncate font-medium text-ink">{disk.name}</span>
                      <span className="shrink-0 font-medium text-ink-soft tabular-nums">
                        {percent(disk.disk_percent)} · {formatBytes(disk.disk_used_bytes)}
                      </span>
                    </div>
                    <ThresholdMeter
                      value={disk.disk_percent}
                      warn={80}
                      danger={90}
                      marks={false}
                      label={t("Disk")}
                    />
                    <p className="text-xs text-muted">{formatTime(disk.sampled_at)}</p>
                  </li>
                ))}
              </ul>
              {metrics.disks.length === 0 ? <Empty>{t("No servers yet.")}</Empty> : null}
            </Card>
            <Card title={t("Backup health by job")}>
              <ul className="space-y-3">
                {ops.backup_jobs.map((job) => (
                  <li
                    key={job.job_id}
                    className="min-w-0 border-b border-line pb-3 text-sm last:border-b-0 last:pb-0"
                  >
                    <div className="flex items-center justify-between gap-2">
                      <p className="truncate font-medium text-ink">{job.name}</p>
                      <Badge
                        value={
                          job.failures > 0
                            ? t("{count} failures", { count: job.failures })
                            : t("Healthy")
                        }
                        tone={job.failures > 0 ? "danger" : "success"}
                      />
                    </div>
                    <p className="mt-1 text-xs text-muted tabular-nums">
                      {t("{runs} runs · {failures} failures", {
                        runs: job.runs,
                        failures: job.failures,
                      })}{" "}
                      ·{" "}
                      {t("Last successful backup: {time}", {
                        time: formatTime(job.last_success),
                      })}
                    </p>
                  </li>
                ))}
              </ul>
              {ops.backup_jobs.length === 0 ? <Empty>{t("No backup jobs yet.")}</Empty> : null}
            </Card>
          </div>

          <p className="break-words text-xs text-muted">
            {t("Configured metric retention: raw {raw}h, 1m {m1}d, 5m {m5}d, 1h {h1}d.", {
              raw: data.retention.raw_hours,
              m1: data.retention.m1_days,
              m5: data.retention.m5_days,
              h1: data.retention.h1_days,
            })}
          </p>
        </>
      ) : null}
    </div>
  );
}

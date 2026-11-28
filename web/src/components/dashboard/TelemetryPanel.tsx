import { useQuery } from "@tanstack/react-query";
import { useId, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api } from "../../api";
import { formatBytes } from "../../format";
import { useT } from "../../i18n";
import type { Schemas } from "../../types";
import { Icon } from "../icons";
import { AXIS_LINE, formatValue, TICK, TOOLTIP_STYLES, type Range } from "../metrics";
import { Empty, Skeleton } from "../ui";

type Tab = "compute" | "traffic" | "network";
type Server = Schemas["ServerView"];

function tickTime(value: number, range: Range): string {
  const date = new Date(value);
  return range === "7d" || range === "30d"
    ? date.toLocaleDateString(undefined, { month: "short", day: "numeric" })
    : date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/**
 * Surgical, zero-bloat Telemetry Panel:
 * - Single-row header with inline underline tabs + subtle server selector.
 * - Inline metric readout bar acts as the live legend (eliminating duplicate bottom legends).
 * - Smooth area gradient with subtle horizontal hairlines.
 */
export function TelemetryPanel({
  servers,
  selectedServerId,
  onSelectServer,
  range,
  paused,
  stats,
  statsLoading,
}: {
  servers: Server[];
  selectedServerId: string | null;
  onSelectServer: (id: string) => void;
  range: Range;
  paused: boolean;
  stats: Schemas["StatsOverview"] | undefined;
  statsLoading: boolean;
}) {
  const t = useT();
  const [tab, setTab] = useState<Tab>("compute");
  const grad1 = useId();
  const grad2 = useId();

  const activeServer = useMemo(
    () => servers.find((s) => s.id === selectedServerId) ?? servers[0] ?? null,
    [servers, selectedServerId],
  );

  const metricsList =
    tab === "compute" ? ["cpu_percent", "memory_percent"] : ["net_rx_bps", "net_tx_bps"];

  const serverMetrics = useQuery({
    queryKey: ["metrics", { server_id: activeServer?.id }, metricsList, range],
    queryFn: () => {
      const params = new URLSearchParams({
        metrics: metricsList.join(","),
        range,
      });
      if (activeServer?.id) {
        params.set("server_id", activeServer.id);
      }
      return api<Schemas["MetricsView"]>(`/api/v1/metrics?${params.toString()}`);
    },
    enabled: tab !== "traffic" && Boolean(activeServer?.id),
    refetchInterval: paused ? false : 10_000,
  });

  const serverPoints = useMemo(() => {
    const rows = new Map<number, { t: number } & Record<string, number>>();
    for (const series of serverMetrics.data?.series ?? []) {
      for (const point of series.points) {
        const time = new Date(point.t).getTime();
        const row = rows.get(time) ?? ({ t: time } as { t: number } & Record<string, number>);
        row[series.metric] = point.avg;
        rows.set(time, row);
      }
    }
    return [...rows.values()].sort((a, b) => a.t - b.t);
  }, [serverMetrics.data]);

  const trafficPoints = useMemo(() => {
    return (stats?.metrics?.series ?? [])
      .map((point) => ({
        t: new Date(point.at).getTime(),
        requests: point.requests ?? 0,
        http_5xx: point.http_5xx ?? 0,
      }))
      .sort((a, b) => a.t - b.t);
  }, [stats]);

  const latestCompute = serverPoints[serverPoints.length - 1];
  const latestNet = serverPoints[serverPoints.length - 1];
  const loading = tab === "traffic" ? statsLoading : serverMetrics.isLoading;
  const points = tab === "traffic" ? trafficPoints : serverPoints;

  const tabs: [Tab, string][] = [
    ["compute", t("Compute")],
    ["traffic", t("Traffic & 5xx")],
    ["network", t("Network")],
  ];

  const resolution =
    tab === "traffic" ? stats?.resolution : serverMetrics.data?.resolution;

  return (
    <section className="overflow-hidden rounded-card border border-line bg-surface">
      {/* Single-row architectural header with underline tabs */}
      <header className="flex flex-wrap items-center justify-between gap-x-4 border-b border-line px-4 sm:px-5">
        <div role="tablist" aria-label={t("Telemetry view")} className="-mb-px flex items-center gap-5">
          {tabs.map(([key, label]) => {
            const active = tab === key;
            return (
              <button
                key={key}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => setTab(key)}
                className={`relative py-3 text-[13.5px] font-medium transition-colors ${
                  active
                    ? "text-ink after:absolute after:right-0 after:bottom-0 after:left-0 after:h-[2px] after:bg-pine"
                    : "text-muted hover:text-ink"
                }`}
              >
                {label}
              </button>
            );
          })}
        </div>

        <div className="flex items-center gap-3 py-2">
          {tab !== "traffic" && activeServer ? (
            servers.length > 1 ? (
              <select
                aria-label={t("Select server")}
                value={activeServer.id}
                onChange={(e) => onSelectServer(e.target.value)}
                className="h-7 cursor-pointer rounded-control border border-transparent bg-sunken/70 px-2 font-mono text-xs text-ink-soft outline-none transition-colors hover:border-line-strong focus:border-pine"
              >
                {servers.map((server) => (
                  <option key={server.id} value={server.id}>
                    {server.name}
                  </option>
                ))}
              </select>
            ) : (
              <span className="font-mono text-xs text-muted">{activeServer.name}</span>
            )
          ) : tab === "traffic" ? (
            <span className="text-xs text-muted">{t("All edge proxies")}</span>
          ) : null}

          <Link
            to={tab === "traffic" ? "/statistics" : "/monitoring"}
            className="inline-flex items-center gap-1 text-xs font-medium text-muted transition-colors hover:text-ink"
          >
            <span className="hidden sm:inline">{t("Explore")}</span>
            <Icon name="arrowRight" size={13} />
          </Link>
        </div>
      </header>

      {/* Inline live readout bar — serves as both live KPI and chart legend */}
      <div className="flex flex-wrap items-baseline justify-between gap-4 px-4 pt-4 pb-1 sm:px-5">
        {tab === "compute" ? (
          <div className="flex flex-wrap items-baseline gap-6 tabular-nums">
            <LegendReadout
              color="var(--chart-1)"
              label={t("CPU")}
              value={formatValue(latestCompute?.cpu_percent, "percent")}
            />
            <LegendReadout
              color="var(--chart-2)"
              label={t("Memory")}
              value={formatValue(latestCompute?.memory_percent, "percent")}
            />
          </div>
        ) : tab === "traffic" ? (
          <div className="flex flex-wrap items-baseline gap-6 tabular-nums">
            <LegendReadout
              color="var(--chart-1)"
              label={t("Requests")}
              value={formatValue(stats?.metrics?.totals.requests, "count")}
            />
            <LegendReadout
              color="var(--chart-4)"
              label={t("HTTP 5xx")}
              value={formatValue(stats?.metrics?.totals.http_5xx, "count")}
            />
          </div>
        ) : (
          <div className="flex flex-wrap items-baseline gap-6 tabular-nums">
            <LegendReadout
              color="var(--chart-1)"
              label={t("Received")}
              value={formatValue(latestNet?.net_rx_bps, "bps")}
            />
            <LegendReadout
              color="var(--chart-3)"
              label={t("Sent")}
              value={formatValue(latestNet?.net_tx_bps, "bps")}
            />
          </div>
        )}
        <span className="font-mono text-[11px] text-faint tabular-nums">
          {resolution ? `${range} · ${resolution}` : range}
        </span>
      </div>

      {/* Chart Canvas */}
      <div className="px-2 pt-2 pb-3 sm:px-4">
        {loading && points.length === 0 ? (
          <div className="px-2 py-2">
            <Skeleton className="h-[244px] w-full rounded-control" />
          </div>
        ) : points.length === 0 ? (
          <div className="flex h-[244px] items-center justify-center">
            <Empty icon="activity" title={t("No samples in this range yet.")}>
              {t("Telemetry samples appear automatically every 10 seconds.")}
            </Empty>
          </div>
        ) : (
          <ResponsiveContainer width="100%" height={244}>
            <AreaChart data={points} margin={{ top: 8, right: 12, bottom: 2, left: 0 }}>
              <defs>
                <linearGradient id={grad1} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="var(--chart-1)" stopOpacity={0.18} />
                  <stop offset="100%" stopColor="var(--chart-1)" stopOpacity={0} />
                </linearGradient>
                <linearGradient id={grad2} x1="0" y1="0" x2="0" y2="1">
                  <stop
                    offset="0%"
                    stopColor={
                      tab === "traffic"
                        ? "var(--chart-4)"
                        : tab === "network"
                          ? "var(--chart-3)"
                          : "var(--chart-2)"
                    }
                    stopOpacity={0.16}
                  />
                  <stop
                    offset="100%"
                    stopColor={
                      tab === "traffic"
                        ? "var(--chart-4)"
                        : tab === "network"
                          ? "var(--chart-3)"
                          : "var(--chart-2)"
                    }
                    stopOpacity={0}
                  />
                </linearGradient>
              </defs>
              <CartesianGrid
                strokeDasharray="2 4"
                stroke="var(--line)"
                strokeOpacity={0.85}
                vertical={false}
              />
              <XAxis
                dataKey="t"
                type="number"
                scale="time"
                domain={["dataMin", "dataMax"]}
                tickFormatter={(value: number) => tickTime(value, range)}
                tick={TICK}
                axisLine={AXIS_LINE}
                tickLine={false}
                minTickGap={42}
              />
              <YAxis
                width={54}
                tick={TICK}
                axisLine={false}
                tickLine={false}
                domain={tab === "compute" ? [0, 100] : [0, "auto"]}
                tickFormatter={(value: number) =>
                  tab === "compute"
                    ? `${Math.round(value)}%`
                    : tab === "network"
                      ? `${formatBytes(value)}/s`
                      : Math.round(value).toLocaleString()
                }
              />
              <Tooltip
                {...TOOLTIP_STYLES}
                labelFormatter={(value) => new Date(Number(value)).toLocaleString()}
                formatter={(value, name) => {
                  const key = String(name);
                  if (tab === "compute") {
                    return [
                      formatValue(Number(value), "percent"),
                      key === "cpu_percent" ? t("CPU") : t("Memory"),
                    ];
                  }
                  if (tab === "traffic") {
                    return [
                      formatValue(Number(value), "count"),
                      key === "requests" ? t("Requests") : t("HTTP 5xx"),
                    ];
                  }
                  return [
                    formatValue(Number(value), "bps"),
                    key === "net_rx_bps" ? t("Received") : t("Sent"),
                  ];
                }}
              />
              {tab === "compute" ? (
                <>
                  <Area
                    type="monotone"
                    dataKey="cpu_percent"
                    stroke="var(--chart-1)"
                    fill={`url(#${grad1})`}
                    strokeWidth={1.75}
                    isAnimationActive={false}
                    connectNulls
                  />
                  <Area
                    type="monotone"
                    dataKey="memory_percent"
                    stroke="var(--chart-2)"
                    fill={`url(#${grad2})`}
                    strokeWidth={1.75}
                    isAnimationActive={false}
                    connectNulls
                  />
                </>
              ) : tab === "traffic" ? (
                <>
                  <Area
                    type="monotone"
                    dataKey="requests"
                    stroke="var(--chart-1)"
                    fill={`url(#${grad1})`}
                    strokeWidth={1.75}
                    isAnimationActive={false}
                    connectNulls
                  />
                  <Area
                    type="monotone"
                    dataKey="http_5xx"
                    stroke="var(--chart-4)"
                    fill={`url(#${grad2})`}
                    strokeWidth={1.75}
                    isAnimationActive={false}
                    connectNulls
                  />
                </>
              ) : (
                <>
                  <Area
                    type="monotone"
                    dataKey="net_rx_bps"
                    stroke="var(--chart-1)"
                    fill={`url(#${grad1})`}
                    strokeWidth={1.75}
                    isAnimationActive={false}
                    connectNulls
                  />
                  <Area
                    type="monotone"
                    dataKey="net_tx_bps"
                    stroke="var(--chart-3)"
                    fill={`url(#${grad2})`}
                    strokeWidth={1.75}
                    isAnimationActive={false}
                    connectNulls
                  />
                </>
              )}
            </AreaChart>
          </ResponsiveContainer>
        )}
      </div>
    </section>
  );
}

function LegendReadout({
  color,
  label,
  value,
}: {
  color: string;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="h-3.5 w-[3px] rounded-full" style={{ backgroundColor: color }} aria-hidden="true" />
      <span className="text-xs text-muted">{label}</span>
      <span className="text-[15px] font-semibold tracking-[-0.02em] text-ink">{value}</span>
    </div>
  );
}

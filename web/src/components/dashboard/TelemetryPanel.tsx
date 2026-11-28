import { useQuery } from "@tanstack/react-query";
import { useId, useMemo, useState } from "react";
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
import { api } from "../../api";
import { formatBytes } from "../../format";
import { useT } from "../../i18n";
import type { Schemas } from "../../types";
import { Icon } from "../icons";
import { AXIS_LINE, formatValue, TICK, TOOLTIP_STYLES, type Range } from "../metrics";
import { buttonClass, Empty, Segmented, Skeleton } from "../ui";

type Tab = "compute" | "traffic" | "network";
type Server = Schemas["ServerView"];

function tickTime(value: number, range: Range): string {
  const date = new Date(value);
  return range === "7d" || range === "30d"
    ? date.toLocaleDateString(undefined, { month: "short", day: "numeric" })
    : date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/**
 * Interactive centerpiece telemetry chart on the Dashboard.
 * Supports switching between Compute (CPU/RAM), Traffic & HTTP 5xx, and Network I/O,
 * cross-filtered by the selected server and synced with the global time range & live polling.
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

  const metricsList = tab === "compute" ? ["cpu_percent", "memory_percent"] : ["net_rx_bps", "net_tx_bps"];

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

  return (
    <section className="overflow-hidden rounded-card border border-line bg-surface shadow-card">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-line px-4 py-3.5 sm:px-5">
        <div className="flex min-w-0 flex-wrap items-center gap-2.5">
          <h2 className="text-[15px] leading-snug font-semibold text-ink">{t("Live telemetry")}</h2>
          {tab !== "traffic" && activeServer ? (
            servers.length > 1 ? (
              <select
                aria-label={t("Select server")}
                value={activeServer.id}
                onChange={(e) => onSelectServer(e.target.value)}
                className="h-7 rounded-control border border-line-strong bg-paper px-2 text-xs font-medium text-ink outline-none focus:border-pine"
              >
                {servers.map((server) => (
                  <option key={server.id} value={server.id}>
                    {server.name}
                  </option>
                ))}
              </select>
            ) : (
              <span className="inline-flex items-center gap-1.5 rounded-full border border-line bg-paper px-2.5 py-0.5 text-xs font-medium text-ink-soft">
                <Icon name="server" size={12} className="text-faint" />
                {activeServer.name}
              </span>
            )
          ) : tab === "traffic" ? (
            <span className="inline-flex items-center gap-1.5 rounded-full border border-line bg-paper px-2.5 py-0.5 text-xs font-medium text-ink-soft">
              <Icon name="globe" size={12} className="text-faint" />
              {t("All edge proxies")}
            </span>
          ) : null}
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Segmented<Tab>
            label={t("Telemetry view")}
            value={tab}
            onChange={setTab}
            options={[
              ["compute", t("Compute")],
              ["traffic", t("Traffic & 5xx")],
              ["network", t("Network")],
            ]}
          />
          <Link
            to={tab === "traffic" ? "/statistics" : "/monitoring"}
            className={buttonClass({ variant: "ghost", size: "sm" })}
          >
            <span className="hidden sm:inline">{t("Explore")}</span>
            <Icon name="arrowRight" size={14} />
          </Link>
        </div>
      </header>

      {/* Summary strip inside chart card */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line/70 bg-paper/45 px-4 py-2 text-xs sm:px-5">
        {tab === "compute" ? (
          <div className="flex flex-wrap items-center gap-4 tabular-nums">
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-full bg-[var(--chart-1)]" />
              <span className="text-muted">{t("CPU")}:</span>
              <span className="font-semibold text-ink">
                {formatValue(latestCompute?.cpu_percent, "percent")}
              </span>
            </span>
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-full bg-[var(--chart-2)]" />
              <span className="text-muted">{t("Memory")}:</span>
              <span className="font-semibold text-ink">
                {formatValue(latestCompute?.memory_percent, "percent")}
              </span>
            </span>
          </div>
        ) : tab === "traffic" ? (
          <div className="flex flex-wrap items-center gap-4 tabular-nums">
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-full bg-[var(--chart-1)]" />
              <span className="text-muted">{t("Requests")}:</span>
              <span className="font-semibold text-ink">
                {formatValue(stats?.metrics?.totals.requests, "count")}
              </span>
            </span>
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-full bg-[var(--chart-4)]" />
              <span className="text-muted">{t("HTTP 5xx")}:</span>
              <span className="font-semibold text-ink">
                {formatValue(stats?.metrics?.totals.http_5xx, "count")}
              </span>
            </span>
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-4 tabular-nums">
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-full bg-[var(--chart-1)]" />
              <span className="text-muted">{t("Received")}:</span>
              <span className="font-semibold text-ink">
                {formatValue(latestNet?.net_rx_bps, "bps")}
              </span>
            </span>
            <span className="flex items-center gap-1.5">
              <span className="size-2 rounded-full bg-[var(--chart-3)]" />
              <span className="text-muted">{t("Sent")}:</span>
              <span className="font-semibold text-ink">
                {formatValue(latestNet?.net_tx_bps, "bps")}
              </span>
            </span>
          </div>
        )}
        <span className="text-faint tabular-nums">
          {tab === "traffic"
            ? stats?.resolution
              ? t("resolution {value}", { value: stats.resolution })
              : range
            : serverMetrics.data?.resolution
              ? t("resolution {value}", { value: serverMetrics.data.resolution })
              : range}
        </span>
      </div>

      <div className="px-3 pt-4 pb-3 sm:px-4">
        {loading && points.length === 0 ? (
          <Skeleton className="h-[250px] w-full rounded-control" />
        ) : points.length === 0 ? (
          <div className="flex h-[250px] items-center justify-center">
            <Empty icon="activity" title={t("No samples in this range yet.")}>
              {t("Telemetry samples appear automatically every 10 seconds.")}
            </Empty>
          </div>
        ) : (
          <ResponsiveContainer width="100%" height={250}>
            <AreaChart data={points} margin={{ top: 6, right: 10, bottom: 0, left: 0 }}>
              <defs>
                <linearGradient id={grad1} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="5%" stopColor="var(--chart-1)" stopOpacity={0.24} />
                  <stop offset="95%" stopColor="var(--chart-1)" stopOpacity={0.01} />
                </linearGradient>
                <linearGradient id={grad2} x1="0" y1="0" x2="0" y2="1">
                  <stop
                    offset="5%"
                    stopColor={
                      tab === "traffic"
                        ? "var(--chart-4)"
                        : tab === "network"
                          ? "var(--chart-3)"
                          : "var(--chart-2)"
                    }
                    stopOpacity={0.22}
                  />
                  <stop
                    offset="95%"
                    stopColor={
                      tab === "traffic"
                        ? "var(--chart-4)"
                        : tab === "network"
                          ? "var(--chart-3)"
                          : "var(--chart-2)"
                    }
                    stopOpacity={0.01}
                  />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke="var(--line)" vertical={false} />
              <XAxis
                dataKey="t"
                type="number"
                scale="time"
                domain={["dataMin", "dataMax"]}
                tickFormatter={(value: number) => tickTime(value, range)}
                tick={TICK}
                axisLine={AXIS_LINE}
                tickLine={false}
                minTickGap={36}
              />
              <YAxis
                width={68}
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
              <Legend
                formatter={(value: string) => {
                  if (value === "cpu_percent") return t("CPU");
                  if (value === "memory_percent") return t("Memory");
                  if (value === "requests") return t("Requests");
                  if (value === "http_5xx") return t("HTTP 5xx");
                  if (value === "net_rx_bps") return t("Received");
                  if (value === "net_tx_bps") return t("Sent");
                  return value;
                }}
                wrapperStyle={{ fontSize: 12, color: "var(--ink-soft)" }}
              />
              {tab === "compute" ? (
                <>
                  <Area
                    type="monotone"
                    dataKey="cpu_percent"
                    stroke="var(--chart-1)"
                    fill={`url(#${grad1})`}
                    strokeWidth={1.9}
                    isAnimationActive={false}
                    connectNulls
                  />
                  <Area
                    type="monotone"
                    dataKey="memory_percent"
                    stroke="var(--chart-2)"
                    fill={`url(#${grad2})`}
                    strokeWidth={1.9}
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
                    strokeWidth={1.9}
                    isAnimationActive={false}
                    connectNulls
                  />
                  <Area
                    type="monotone"
                    dataKey="http_5xx"
                    stroke="var(--chart-4)"
                    fill={`url(#${grad2})`}
                    strokeWidth={1.9}
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
                    strokeWidth={1.9}
                    isAnimationActive={false}
                    connectNulls
                  />
                  <Area
                    type="monotone"
                    dataKey="net_tx_bps"
                    stroke="var(--chart-3)"
                    fill={`url(#${grad2})`}
                    strokeWidth={1.9}
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

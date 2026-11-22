import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
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
import { AXIS_LINE, COLORS, TICK, TOOLTIP_STYLES } from "../components/metrics";
import { Button, Card, Empty } from "../components/ui";
import { PanelBackupStatus } from "../components/panel-backups";
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
const number = (value: number | null | undefined) =>
  value == null ? "—" : Math.round(value).toLocaleString();
const percent = (value: number | null | undefined) =>
  value == null ? "—" : `${value.toFixed(1)}%`;
const rate = (value: number | null | undefined) =>
  value == null ? "—" : percent(value * 100);

export function StatisticsPage() {
  const t = useT();
  const [range, setRange] = useState("1h");
  const query = useQuery({
    queryKey: ["statistics", range],
    queryFn: () =>
      api<Schemas["StatsOverview"]>(`/api/v1/stats/overview?range=${range}`),
    refetchInterval: 60000,
  });
  const data = query.data;
  const metrics = data?.metrics;
  const totals = metrics?.totals;
  const ops = data?.operations;
  const cards =
    totals && ops
      ? [
          [t("Servers"), number(ops.counts.servers)],
          [t("Services"), number(ops.counts.services)],
          [t("Domains"), number(ops.counts.domains)],
          [t("Plugins"), number(ops.counts.plugins)],
          [t("Network received"), formatBytes(totals.rx_bytes)],
          [t("Network sent"), formatBytes(totals.tx_bytes)],
          [t("Proxy requests"), number(totals.requests)],
          [t("Proxy 5xx"), number(totals.http_5xx)],
          [t("Proxy 5xx rate"), rate(totals.http_5xx_rate)],
          [t("Average server CPU"), percent(totals.cpu_avg)],
          [t("Peak server CPU"), percent(totals.cpu_peak)],
          [t("Average server memory"), percent(totals.memory_avg)],
          [t("Peak server memory"), percent(totals.memory_peak)],
          [t("Deploys in range"), number(ops.deploys)],
          [t("Deploy success rate"), rate(ops.deploy_success_rate)],
          [
            t("Average deploy duration"),
            formatDuration(
              ops.deploy_avg_duration_seconds == null
                ? null
                : ops.deploy_avg_duration_seconds * 1000,
            ),
          ],
          [t("Backup runs in range"), number(ops.backup_runs)],
          [t("Backup failures in range"), number(ops.backup_failures)],
          [
            t("Observed restart increases"),
            number(metrics?.container_restarts),
          ],
          [t("Alerts fired in range"), number(ops.alerts_fired)],
          [t("MCP calls"), number(ops.mcp_calls)],
          [t("MCP error rate"), rate(ops.mcp_error_rate)],
          [t("Denied MCP calls"), number(ops.mcp_denied)],
        ]
      : [];
  const top = (
    title: string,
    rows: Top[],
    metric: Metric,
    format: (value: number | null | undefined) => string,
  ) => (
    <Card title={title}>
      {rows.length ? (
        <ol className="space-y-3">
          {rows.map((row, i) => (
            <li
              key={row.service_id}
              className="flex min-w-0 justify-between gap-3 text-sm"
            >
              <Link
                className="min-w-0 break-all text-pine hover:underline"
                to={`/services/${row.service_id}`}
              >
                {i + 1}. {row.name}
              </Link>
              <span className="shrink-0 tabular-nums">
                {format(row[metric])}
              </span>
            </li>
          ))}
        </ol>
      ) : (
        <Empty>{t("No statistics available in this range.")}</Empty>
      )}
    </Card>
  );
  const chart = (
    title: string,
    lines: [keyof Point, string][],
    format: (value: number | null | undefined) => string,
  ) => (
    <Card title={title}>
      {metrics?.series.some((point) =>
        lines.some(([key]) => point[key] != null),
      ) ? (
        <ResponsiveContainer width="100%" height={240}>
          <AreaChart data={metrics.series} margin={{ left: 0, right: 10 }}>
            <CartesianGrid
              stroke="var(--line)"
              vertical={false}
              strokeDasharray="3 3"
            />
            <XAxis
              dataKey="at"
              tick={TICK}
              axisLine={AXIS_LINE}
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
              axisLine={AXIS_LINE}
              tickFormatter={(value: number) => format(value)}
            />
            <Tooltip
              {...TOOLTIP_STYLES}
              labelFormatter={(value) =>
                new Date(String(value)).toLocaleString()
              }
              formatter={(value, name) => [format(Number(value)), name]}
            />
            <Legend />
            {lines.map(([key, name], i) => (
              <Area
                key={key}
                dataKey={key}
                name={name}
                stroke={COLORS[i]}
                fill={COLORS[i]}
                fillOpacity={0.12}
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
  return (
    <div className="min-w-0 space-y-5">
      {data ? (
        <Card title={t("Panel backups")}>
          <PanelBackupStatus health={data.panel_backup} />
          <Link className="inline-flex min-h-10 items-center text-accent" to="/system#panel-backups">
            {t("Manage panel backups")}
          </Link>
        </Card>
      ) : null}
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold">{t("Statistics")}</h1>
          <p className="mt-1 text-sm text-muted">
            {t("Traffic, resources, and operations across Rudder.")}
          </p>
        </div>
        <div
          className="flex flex-wrap gap-2"
          aria-label={t("Statistics range")}
        >
          {["1h", "24h", "7d", "30d"].map((value) => (
            <Button
              key={value}
              quiet={range !== value}
              aria-pressed={range === value}
              onClick={() => setRange(value)}
            >
              {value}
            </Button>
          ))}
        </div>
      </header>
      <p className="text-sm text-muted">
        {t(
          "Totals are estimates from retained samples. Missing metrics show —. Restart increases may miss resets and container replacements.",
        )}
      </p>
      {query.isPending ? <p role="status">{t("Loading statistics…")}</p> : null}
      {query.isError ? (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-3 break-words text-danger"
        >
          <span className="min-w-0 flex-1 break-words">
            {query.error.message}
          </span>
          <Button quiet onClick={() => query.refetch()}>
            {t("Retry")}
          </Button>
        </div>
      ) : null}
      {data && metrics && ops ? (
        <>
          <p className="break-words text-xs text-muted">
            {formatTime(data.since)} — {formatTime(data.until)} ·{" "}
            {t("Metric resolution: {resolution}", {
              resolution: data.resolution,
            })}
          </p>
          <div className="grid min-w-0 grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
            {cards.map(([label, value]) => (
              <div
                className="min-w-0 rounded-lg border border-line bg-surface p-3"
                key={label}
              >
                <p className="break-words text-xs text-muted">{label}</p>
                <p className="mt-1 break-words text-xl font-semibold tabular-nums">
                  {value}
                </p>
              </div>
            ))}
          </div>
          <div className="grid min-w-0 gap-4 xl:grid-cols-2">
            {chart(
              t("Network volume per bucket"),
              [
                ["rx_bytes", t("Network received")],
                ["tx_bytes", t("Network sent")],
              ],
              formatBytes,
            )}
            {chart(
              t("Proxy requests per bucket"),
              [
                ["requests", t("Proxy requests")],
                ["http_5xx", t("Proxy 5xx")],
              ],
              number,
            )}
            {chart(
              t("Server CPU over time"),
              [
                ["cpu_avg", t("Average server CPU")],
                ["cpu_peak", t("Peak server CPU")],
              ],
              percent,
            )}
            {chart(
              t("Proxy 5xx rate"),
              [["http_5xx_rate", t("Proxy 5xx rate")]],
              rate,
            )}
            {chart(
              t("Server memory over time"),
              [
                ["memory_avg", t("Average server memory")],
                ["memory_peak", t("Peak server memory")],
              ],
              percent,
            )}
          </div>
          <div className="grid min-w-0 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {top(
              t("Top services by network volume"),
              metrics.top_network,
              "network_bytes",
              formatBytes,
            )}
            {top(
              t("Top services by requests"),
              metrics.top_requests,
              "requests",
              number,
            )}
            {top(t("Top services by 5xx"), metrics.top_5xx, "http_5xx", number)}
            {top(
              t("Top services by CPU"),
              metrics.top_cpu,
              "cpu_percent",
              percent,
            )}
            {top(
              t("Top services by memory"),
              metrics.top_memory,
              "memory_bytes",
              formatBytes,
            )}
          </div>
          <div className="grid min-w-0 gap-4 lg:grid-cols-2">
            <Card title={t("Latest disk usage by server")}>
              <ul className="space-y-4">
                {metrics.disks.map((disk) => (
                  <li
                    key={disk.server_id}
                    className="min-w-0 border-b border-line pb-3 text-sm"
                  >
                    <p className="break-all font-medium">{disk.name}</p>
                    <p>
                      {percent(disk.disk_percent)} ·{" "}
                      {formatBytes(disk.disk_used_bytes)}
                    </p>
                    <p className="break-words text-xs text-muted">
                      {formatTime(disk.sampled_at)}
                    </p>
                  </li>
                ))}
              </ul>
              {metrics.disks.length === 0 ? (
                <Empty>{t("No servers yet.")}</Empty>
              ) : null}
            </Card>
            <Card title={t("Backup health by job")}>
              <ul className="space-y-4">
                {ops.backup_jobs.map((job) => (
                  <li
                    key={job.job_id}
                    className="min-w-0 border-b border-line pb-3 text-sm"
                  >
                    <p className="break-all font-medium">{job.name}</p>
                    <p>
                      {t("{runs} runs · {failures} failures", {
                        runs: job.runs,
                        failures: job.failures,
                      })}
                    </p>
                    <p className="break-words text-xs text-muted">
                      {t("Last successful backup: {time}", {
                        time: formatTime(job.last_success),
                      })}
                    </p>
                  </li>
                ))}
              </ul>
              {ops.backup_jobs.length === 0 ? (
                <Empty>{t("No backup jobs yet.")}</Empty>
              ) : null}
            </Card>
          </div>
          <p className="break-words text-xs text-muted">
            {t(
              "Configured metric retention: raw {raw}h, 1m {m1}d, 5m {m5}d, 1h {h1}d.",
              {
                raw: data.retention.raw_hours,
                m1: data.retention.m1_days,
                m5: data.retention.m5_days,
                h1: data.retention.h1_days,
              },
            )}
          </p>
        </>
      ) : null}
    </div>
  );
}

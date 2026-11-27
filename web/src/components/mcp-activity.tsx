import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api } from "../api";
import { useT } from "../i18n";
import type { Schemas } from "../types";
import { AXIS_LINE, COLORS, TICK, TOOLTIP_STYLES } from "./metrics";
import type { PermissionCatalog } from "./mcp-permissions";
import { Badge, Button, Card, Empty } from "./ui";

interface Client {
  id: string;
  client_id: string;
  client_name: string;
  name: string;
}
type Filter = "client_id" | "tool" | "status" | "days" | "bucket";

export function McpActivity({
  grants,
  tools,
}: {
  grants: Client[];
  tools: PermissionCatalog["tools"];
}) {
  const t = useT();
  const [filters, setFilters] = useState({
    client_id: "",
    tool: "",
    status: "",
    days: "7",
    bucket: "hour",
  });
  const [offset, setOffset] = useState(0);
  const params = new URLSearchParams(
    Object.entries(filters).filter(([, value]) => value !== ""),
  ).toString();
  const stats = useQuery({
    queryKey: ["mcp-activity", "stats", params],
    queryFn: () => api<Schemas["McpUsageStats"]>(`/api/v1/mcp/stats?${params}`),
    refetchInterval: 30000,
  });
  const calls = useQuery({
    queryKey: ["mcp-activity", "calls", params, offset],
    queryFn: () =>
      api<Schemas["McpCall"][]>(
        `/api/v1/mcp/calls?${params}&limit=50&offset=${offset}`,
      ),
    refetchInterval: 30000,
  });
  const clients = new Map(
    grants.map((grant) => [grant.client_id, grant.client_name]),
  );
  for (const client of stats.data?.clients ?? [])
    if (client.client_id && !clients.has(client.client_id))
      clients.set(client.client_id, client.client_id);
  const clientLabel = (id: string | null | undefined) =>
    id ? (clients.get(id) ?? id) : t("Unknown MCP client");
  const select = (key: Filter, label: string, options: [string, string][]) => (
    <label className="grid min-w-0 gap-1 text-sm">
      {label}
      <select
        className="min-h-10 w-full min-w-0 rounded-md border border-line bg-paper px-2"
        value={filters[key]}
        onChange={(event) => {
          setFilters((current) => ({ ...current, [key]: event.target.value }));
          setOffset(0);
        }}
      >
        {options.map(([value, label]) => (
          <option key={value} value={value}>
            {label}
          </option>
        ))}
      </select>
    </label>
  );
  const ms = (value: number | null | undefined) =>
    value == null ? "—" : t("{ms} ms", { ms: Math.round(value) });
  const summary = stats.data?.summary;
  return (
    <div className="min-w-0 space-y-4">
      <Card title={t("AI activity")}>
        <p className="mb-4 text-sm text-muted">
          {t(
            "MCP tool calls are kept for 30 days. Arguments and secrets are never recorded. Activity refreshes every 30 seconds.",
          )}
        </p>
        <div className="grid min-w-0 gap-3 sm:grid-cols-2 xl:grid-cols-5">
          {select("client_id", t("MCP client"), [
            ["", t("All MCP clients")],
            ...[...clients].map(([id, name]): [string, string] => [id, name]),
          ])}
          {select("tool", t("MCP tool"), [
            ["", t("All MCP tools")],
            ["dymini_panel_whoami", "dymini_panel_whoami"],
            ["unknown", t("Unknown MCP tool")],
            ...tools.map((tool): [string, string] => [tool.name, tool.name]),
          ])}
          {select("status", t("Result"), [
            ["", t("All MCP results")],
            ["ok", t("Successful MCP calls")],
            ["denied", t("Denied MCP calls")],
            ["error", t("Failed MCP calls")],
          ])}
          {select("days", t("Time range"), [
            ["1", t("Past 24 hours")],
            ["7", t("Past 7 days")],
            ["30", t("Past 30 days")],
          ])}
          {select("bucket", t("MCP time buckets"), [
            ["hour", t("Hourly (UTC)")],
            ["day", t("Daily (UTC)")],
          ])}
        </div>
        {stats.isError ? (
          <p role="alert" className="mt-3 break-words text-sm text-danger">
            {stats.error.message}
          </p>
        ) : null}
        {stats.isPending ? (
          <p role="status" className="mt-3">
            {t("Loading AI activity…")}
          </p>
        ) : null}
        {summary ? (
          <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
            {[
              [t("MCP calls"), summary.total.toLocaleString()],
              [
                t("MCP error rate"),
                `${(summary.error_rate * 100).toFixed(1)}%`,
              ],
              [
                t("MCP denied rate"),
                `${(summary.denied_rate * 100).toFixed(1)}%`,
              ],
              [t("MCP p50 latency"), ms(summary.p50_ms)],
              [t("MCP p95 latency"), ms(summary.p95_ms)],
            ].map(([label, value]) => (
              <div
                key={label}
                className="min-w-0 rounded-md border border-line p-3"
              >
                <p className="text-xs text-muted">{label}</p>
                <p className="break-words text-xl font-semibold">{value}</p>
              </div>
            ))}
          </div>
        ) : null}
      </Card>
      {stats.data ? (
        <>
          <Card title={t("MCP calls over time")}>
            {summary?.total === 0 ? (
              <Empty>{t("No MCP calls match these filters.")}</Empty>
            ) : (
              <ResponsiveContainer width="100%" height={240}>
                <AreaChart data={stats.data.buckets}>
                  <CartesianGrid
                    stroke="var(--line)"
                    strokeDasharray="3 3"
                    vertical={false}
                  />
                  <XAxis
                    dataKey="at"
                    tick={TICK}
                    axisLine={AXIS_LINE}
                    minTickGap={40}
                    tickFormatter={(value: string) =>
                      new Date(value).toLocaleString(undefined, {
                        month: "short",
                        day: "numeric",
                        ...(filters.bucket === "hour"
                          ? { hour: "2-digit" }
                          : {}),
                      })
                    }
                  />
                  <YAxis
                    allowDecimals={false}
                    width={45}
                    tick={TICK}
                    axisLine={AXIS_LINE}
                  />
                  <Tooltip
                    {...TOOLTIP_STYLES}
                    labelFormatter={(value) =>
                      new Date(String(value)).toLocaleString()
                    }
                  />
                  <Legend />
                  <Area
                    type="monotone"
                    dataKey="ok"
                    stackId="calls"
                    name={t("Successful MCP calls")}
                    stroke={COLORS[0]}
                    fill={COLORS[0]}
                    fillOpacity={0.25}
                  />
                  <Area
                    type="monotone"
                    dataKey="denied"
                    stackId="calls"
                    name={t("Denied MCP calls")}
                    stroke={COLORS[1]}
                    fill={COLORS[1]}
                    fillOpacity={0.25}
                  />
                  <Area
                    type="monotone"
                    dataKey="error"
                    stackId="calls"
                    name={t("Failed MCP calls")}
                    stroke={COLORS[3]}
                    fill={COLORS[3]}
                    fillOpacity={0.25}
                  />
                </AreaChart>
              </ResponsiveContainer>
            )}
          </Card>
          <Card title={t("Top MCP tools")}>
            {stats.data.tools.length ? (
              <ResponsiveContainer
                width="100%"
                height={Math.max(160, stats.data.tools.length * 30)}
              >
                <BarChart
                  data={stats.data.tools}
                  layout="vertical"
                  margin={{ left: 0, right: 12 }}
                >
                  <XAxis
                    type="number"
                    allowDecimals={false}
                    tick={TICK}
                    axisLine={AXIS_LINE}
                  />
                  <YAxis
                    type="category"
                    dataKey="tool"
                    width={165}
                    tick={TICK}
                    axisLine={AXIS_LINE}
                    tickFormatter={(value: string) =>
                      value.length > 24 ? `${value.slice(0, 22)}…` : value
                    }
                  />
                  <Tooltip {...TOOLTIP_STYLES} />
                  <Bar dataKey="count" name={t("MCP calls")} fill={COLORS[0]} />
                </BarChart>
              </ResponsiveContainer>
            ) : (
              <Empty>{t("No MCP calls match these filters.")}</Empty>
            )}
          </Card>
          <div className="grid min-w-0 gap-4 lg:grid-cols-2">
            <Card title={t("MCP calls by client")}>
              <ul className="space-y-2 text-sm">
                {stats.data.clients.map((client) => (
                  <li
                    key={client.client_id ?? "anonymous"}
                    className="flex min-w-0 justify-between gap-3"
                  >
                    <span className="min-w-0 break-all">
                      {clientLabel(client.client_id)}
                    </span>
                    <span>{client.count}</span>
                  </li>
                ))}
              </ul>
            </Card>
            <Card title={t("MCP last used by grant")}>
              <ul className="space-y-3 text-sm">
                {stats.data.grants.map((grant) => (
                  <li key={grant.grant_id}>
                    <p className="break-all font-medium">
                      {grants.find((g) => g.id === grant.grant_id)?.name ??
                        grant.grant_id}
                    </p>
                    <p className="text-muted">
                      {grant.count} ·{" "}
                      {new Date(grant.last_used_at).toLocaleString()}
                    </p>
                  </li>
                ))}
              </ul>
            </Card>
          </div>
        </>
      ) : null}
      <Card title={t("Recent MCP calls")}>
        {calls.isError ? (
          <p role="alert" className="break-words text-sm text-danger">
            {calls.error.message}
          </p>
        ) : null}
        {calls.isPending ? (
          <p role="status">{t("Loading AI activity…")}</p>
        ) : null}
        {calls.data?.length === 0 ? (
          <Empty>{t("No MCP calls match these filters.")}</Empty>
        ) : null}
        {calls.data?.length ? (
          <div className="max-w-full overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b border-line">
                  {[
                    t("Call time"),
                    t("MCP client"),
                    t("MCP tool"),
                    t("MCP scope"),
                    t("Result"),
                    t("HTTP status"),
                    t("Duration"),
                    t("Error summary"),
                  ].map((label) => (
                    <th key={label} className="whitespace-nowrap px-2 py-2">
                      {label}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {calls.data.map((call) => (
                  <tr key={call.id} className="border-b border-line align-top">
                    <td className="whitespace-nowrap p-2">
                      {new Date(call.created_at).toLocaleString()}
                    </td>
                    <td className="p-2">
                      <span className="block w-36 break-all">
                        {clientLabel(call.client_id)}
                      </span>
                    </td>
                    <td className="p-2">
                      <code className="block w-52 break-all text-xs">
                        {call.tool}
                      </code>
                    </td>
                    <td className="whitespace-nowrap p-2">{call.scope}</td>
                    <td className="p-2">
                      <Badge value={call.status} />
                    </td>
                    <td className="p-2">{call.http_status}</td>
                    <td className="whitespace-nowrap p-2">
                      {ms(call.duration_ms)}
                    </td>
                    <td className="p-2">
                      <span className="block w-56 break-words">
                        {call.error_message ?? "—"}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            quiet
            disabled={offset === 0 || calls.isFetching}
            onClick={() => setOffset(Math.max(0, offset - 50))}
          >
            {t("Previous")}
          </Button>
          <Button
            quiet
            disabled={
              (calls.data?.length ?? 0) < 50 ||
              offset >= 100000 ||
              calls.isFetching
            }
            onClick={() => setOffset(offset + 50)}
          >
            {t("Next")}
          </Button>
        </div>
      </Card>
    </div>
  );
}

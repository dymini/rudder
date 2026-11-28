import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { toast } from "sonner";
import { api } from "../api";
import { useAuth } from "../auth";
import { ThresholdMeter } from "../components/load";
import {
  formatValue,
  MetricsGrid,
  RANGES,
  SERVER_CHARTS,
  type Range,
} from "../components/metrics";
import { KpiCard, LiveControlBar } from "../components/ops-ui";
import {
  Badge,
  Button,
  Card,
  Empty,
  Field,
  QuickPick,
  Select,
  StatusDot,
  TextInput,
} from "../components/ui";
import { errorMessage, formatBytes, formatTime } from "../format";
import { msg, useT } from "../i18n";
import { METRIC_RETENTION } from "../presets";
import type { Schemas } from "../types";

type MetricsRetention = Schemas["MetricsRetention"];

const RETENTION_FIELDS: [keyof MetricsRetention, string, number][] = [
  ["raw_hours", msg("10-second samples (hours)"), 168],
  ["m1_days", msg("1-minute rollups (days)"), 365],
  ["m5_days", msg("5-minute rollups (days)"), 365],
  ["h1_days", msg("Hourly rollups (days)"), 365],
];

const retentionKey = (values: Record<string, string>) =>
  RETENTION_FIELDS.map(([key]) => values[key] ?? "").join("|");

function MetricsRetentionCard() {
  const t = useT();
  const queryClient = useQueryClient();
  const retention = useQuery({
    queryKey: ["retention"],
    queryFn: () => api<Schemas["RetentionSettings"]>("/api/v1/settings/retention"),
  });
  const [values, setValues] = useState<Record<string, string>>({});
  useEffect(() => {
    if (retention.data) {
      setValues(
        Object.fromEntries(
          Object.entries(retention.data.metrics).map(([key, value]) => [key, String(value)]),
        ),
      );
    }
  }, [retention.data]);
  const save = useMutation({
    mutationFn: () =>
      api("/api/v1/settings/retention", {
        method: "PUT",
        body: JSON.stringify({
          metrics: Object.fromEntries(RETENTION_FIELDS.map(([key]) => [key, Number(values[key])])),
        }),
      }),
    onSuccess: async () => {
      toast.success(t("Retention saved"));
      await queryClient.invalidateQueries({ queryKey: ["retention"] });
    },
    onError: (error) => toast.error(errorMessage(error, t("Could not save retention"))),
  });
  return (
    <Card title={t("Metric retention")}>
      <form
        className="grid gap-3 md:grid-cols-4"
        onSubmit={(event) => {
          event.preventDefault();
          save.mutate();
        }}
      >
        <div className="space-y-1.5 md:col-span-4">
          <span className="text-[13px] font-medium text-ink-soft">{t("Preset")}</span>
          <QuickPick
            label={t("Metric retention presets")}
            options={METRIC_RETENTION.map((preset) => ({
              label: preset.label,
              value: retentionKey(preset.values),
              hint: preset.hint,
            }))}
            value={retentionKey(values)}
            onPick={(_, option) => {
              const preset = METRIC_RETENTION.find((item) => item.label === option.label);
              if (preset) setValues({ ...values, ...preset.values });
            }}
          />
        </div>
        {RETENTION_FIELDS.map(([key, label, max]) => (
          <Field key={key} label={t(label)}>
            <TextInput
              type="number"
              min={1}
              max={max}
              value={values[key] ?? ""}
              onChange={(event) => setValues({ ...values, [key]: event.target.value })}
            />
          </Field>
        ))}
        <div className="md:col-span-4">
          <Button type="submit" disabled={save.isPending || !retention.data}>
            {t("Save retention")}
          </Button>
        </div>
      </form>
    </Card>
  );
}

export function MonitoringPage() {
  const t = useT();
  const { user } = useAuth();
  const queryClient = useQueryClient();
  const [range, setRange] = useState<Range>("1h");
  const [paused, setPaused] = useState(false);

  const overview = useQuery({
    queryKey: ["metrics-overview"],
    queryFn: () => api<Schemas["MetricsOverview"]>("/api/v1/metrics/overview"),
    refetchInterval: paused ? false : 10_000,
  });

  const servers = overview.data?.servers ?? [];
  const services = useMemo(
    () =>
      [...(overview.data?.services ?? [])].sort(
        (a, b) => (b.cpu_percent ?? 0) - (a.cpu_percent ?? 0),
      ),
    [overview.data],
  );
  const [picked, setPicked] = useState<string | null>(null);
  const serverId = picked ?? servers[0]?.server_id ?? null;
  const serverName = (id: string) => servers.find((s) => s.server_id === id)?.name ?? "—";

  const onlineServers = servers.filter((s) => s.status === "online");
  const avgCpu = onlineServers.length
    ? onlineServers.reduce((sum, s) => sum + (s.cpu_percent ?? 0), 0) / onlineServers.length
    : null;
  const avgMem = onlineServers.length
    ? onlineServers.reduce((sum, s) => sum + (s.memory_percent ?? 0), 0) / onlineServers.length
    : null;
  const peakDisk = onlineServers.length
    ? Math.max(...onlineServers.map((s) => s.disk_percent ?? 0))
    : null;

  const handleRefresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["metrics-overview"] });
    void queryClient.invalidateQueries({ queryKey: ["metrics"] });
  };

  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="text-xl leading-tight font-semibold tracking-[-0.015em] text-ink sm:text-2xl">
            {t("Monitoring")}
          </h1>
          <p className="mt-0.5 text-sm text-muted">
            {t(
              "Sampled every 10 seconds, rolled up to 1-minute, 5-minute, and hourly averages. Configure rules and channels under {link}.",
              { link: <Link to="/alerts">{t("Alerts")}</Link> },
            )}
          </p>
        </div>
        <LiveControlBar
          ranges={RANGES}
          range={range}
          onRange={setRange}
          paused={paused}
          onTogglePause={() => setPaused((p) => !p)}
          onRefresh={handleRefresh}
          refreshing={overview.isFetching}
          updatedAt={overview.dataUpdatedAt}
        />
      </div>

      {/* Fleet KPI Summary */}
      <div className="grid grid-cols-2 gap-3 sm:gap-4 xl:grid-cols-4">
        <KpiCard
          icon="server"
          label={t("Servers online")}
          loading={overview.isLoading}
          value={onlineServers.length}
          suffix={t("of {total}", { total: servers.length })}
          tone={onlineServers.length < servers.length ? "danger" : "pine"}
        />
        <KpiCard
          icon="cpu"
          label={t("Fleet CPU")}
          loading={overview.isLoading}
          value={avgCpu == null ? "—" : `${avgCpu.toFixed(1)}%`}
          tone={avgCpu != null && avgCpu >= 85 ? "danger" : avgCpu != null && avgCpu >= 70 ? "warning" : "default"}
        />
        <KpiCard
          icon="memory"
          label={t("Fleet memory")}
          loading={overview.isLoading}
          value={avgMem == null ? "—" : `${avgMem.toFixed(1)}%`}
          tone={avgMem != null && avgMem >= 85 ? "danger" : avgMem != null && avgMem >= 75 ? "warning" : "default"}
        />
        <KpiCard
          icon="archive"
          label={t("Highest disk")}
          loading={overview.isLoading}
          value={peakDisk == null ? "—" : `${peakDisk.toFixed(1)}%`}
          tone={peakDisk != null && peakDisk >= 90 ? "danger" : peakDisk != null && peakDisk >= 80 ? "warning" : "default"}
        />
      </div>

      <Card
        title={t("Servers")}
        description={t("Click a server row to inspect its time-series metrics below.")}
        flush
      >
        <div className="overflow-x-auto border-t border-line">
          <table className="w-full text-left text-sm">
            <thead className="bg-paper/60 text-xs text-muted">
              <tr>
                <th className="py-2.5 pr-3 pl-5 font-medium sm:pl-6">{t("Server")}</th>
                <th className="py-2.5 pr-3 font-medium">{t("Status")}</th>
                <th className="w-36 py-2.5 pr-4 font-medium">{t("CPU")}</th>
                <th className="w-36 py-2.5 pr-4 font-medium">{t("Memory")}</th>
                <th className="w-36 py-2.5 pr-4 font-medium">{t("Disk")}</th>
                <th className="py-2.5 pr-3 text-right font-medium">{t("Load")}</th>
                <th className="py-2.5 pr-5 text-right font-medium sm:pr-6">{t("Sampled")}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {servers.map((server) => {
                const active = server.server_id === serverId;
                return (
                  <tr
                    key={server.server_id}
                    className={`cursor-pointer transition-colors ${
                      active ? "bg-pine-soft/45" : "hover:bg-paper"
                    }`}
                    onClick={() => setPicked(server.server_id)}
                  >
                    <td className="py-3 pr-3 pl-5 font-medium text-ink sm:pl-6">
                      <span className="flex items-center gap-2">
                        <StatusDot status={server.status} />
                        {server.name}
                      </span>
                    </td>
                    <td className="py-3 pr-3">
                      <Badge value={server.status} />
                    </td>
                    <td className="py-3 pr-4">
                      <InlineMetric value={server.cpu_percent} label={t("CPU")} />
                    </td>
                    <td className="py-3 pr-4">
                      <InlineMetric value={server.memory_percent} label={t("Memory")} />
                    </td>
                    <td className="py-3 pr-4">
                      <InlineMetric value={server.disk_percent} label={t("Disk")} />
                    </td>
                    <td className="py-3 pr-3 text-right tabular-nums">
                      {formatValue(server.load1, "plain")}
                    </td>
                    <td className="py-3 pr-5 text-right text-xs text-muted tabular-nums sm:pr-6">
                      {formatTime(server.sampled_at)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        {servers.length === 0 && !overview.isLoading ? <Empty>{t("No servers yet.")}</Empty> : null}
      </Card>

      {serverId ? (
        <Card
          title={t("Server {name}", { name: serverName(serverId) })}
          actions={
            servers.length > 1 ? (
              <div className="w-48">
                <Select
                  aria-label={t("Server")}
                  value={serverId}
                  onChange={(event) => setPicked(event.target.value)}
                  className="h-8! text-xs!"
                >
                  {servers.map((server) => (
                    <option key={server.server_id} value={server.server_id}>
                      {server.name}
                    </option>
                  ))}
                </Select>
              </div>
            ) : null
          }
        >
          <MetricsGrid
            charts={SERVER_CHARTS}
            target={{ server_id: serverId }}
            range={range}
            onRange={setRange}
            paused={paused}
          />
        </Card>
      ) : null}

      <Card title={t("Services")} flush>
        <div className="overflow-x-auto border-t border-line">
          <table className="w-full text-left text-sm">
            <thead className="bg-paper/60 text-xs text-muted">
              <tr>
                <th className="py-2.5 pr-3 pl-5 font-medium sm:pl-6">{t("Service")}</th>
                <th className="py-2.5 pr-3 font-medium">{t("Server")}</th>
                <th className="w-40 py-2.5 pr-4 font-medium">{t("CPU")}</th>
                <th className="py-2.5 pr-3 text-right font-medium">{t("Memory")}</th>
                <th className="py-2.5 pr-5 text-right font-medium sm:pr-6">{t("Containers up")}</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {services.map((service) => (
                <tr key={service.service_id} className="transition-colors hover:bg-paper">
                  <td className="py-2.5 pr-3 pl-5 font-medium sm:pl-6">
                    <Link
                      to={`/services/${service.service_id}`}
                      className="text-ink hover:text-pine"
                    >
                      {service.name}
                    </Link>
                  </td>
                  <td className="py-2.5 pr-3 text-muted">{serverName(service.server_id)}</td>
                  <td className="py-2.5 pr-4">
                    <InlineMetric value={service.cpu_percent} label={t("CPU")} warn={60} danger={85} />
                  </td>
                  <td className="py-2.5 pr-3 text-right tabular-nums">
                    {service.memory_bytes == null ? "—" : formatBytes(service.memory_bytes)}
                  </td>
                  <td className="py-2.5 pr-5 text-right tabular-nums sm:pr-6">
                    {service.containers ?? "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        {services.length === 0 && !overview.isLoading ? (
          <Empty>{t("No running services yet.")}</Empty>
        ) : null}
      </Card>

      {user?.role === "admin" ? <MetricsRetentionCard /> : null}
    </>
  );
}

function InlineMetric({
  value,
  label,
  warn = 75,
  danger = 90,
}: {
  value: number | null | undefined;
  label: string;
  warn?: number;
  danger?: number;
}) {
  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between text-xs tabular-nums">
        <span className="font-medium text-ink-soft">
          {value == null ? "—" : `${value.toFixed(1)}%`}
        </span>
      </div>
      <ThresholdMeter value={value} warn={warn} danger={danger} marks={false} label={label} />
    </div>
  );
}

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useRef, useState, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import { useAuth } from "../auth";
import { ActivityFeedCard } from "../components/dashboard/ActivityFeedCard";
import {
  AdaptiveHealthStrip,
  type DashboardIssue,
} from "../components/dashboard/AdaptiveHealthStrip";
import { FleetPanel } from "../components/dashboard/FleetPanel";
import { TelemetryPanel } from "../components/dashboard/TelemetryPanel";
import {
  UnifiedServicesCard,
  type ServiceFilter,
} from "../components/dashboard/UnifiedServicesCard";
import { Icon } from "../components/icons";
import { ImageStatusCard } from "../components/image-status";
import { holdText, PanelLoadCard, reasonText, useLoad, useLoadActions } from "../components/load";
import { LiveControlBar, MetricCell, MetricStrip, Sparkline } from "../components/ops-ui";
import { buttonClass, Meter } from "../components/ui";
import { formatRelative } from "../format";
import { isIssue } from "../health";
import { useT } from "../i18n";
import type { Schemas } from "../types";

type Server = Schemas["ServerView"];
type Service = Schemas["ServiceSummary"];
type Event = Schemas["EventView"];
type AlertRule = Schemas["AlertView"];

const DASHBOARD_RANGES = ["15m", "1h", "6h", "24h"] as const;
type DashboardRange = (typeof DASHBOARD_RANGES)[number];

const LIVE_MS = 10_000;

function toStatsRange(range: DashboardRange): "1h" | "24h" {
  return range === "15m" || range === "1h" ? "1h" : "24h";
}

export function DashboardPage() {
  const t = useT();
  const { user } = useAuth();
  const queryClient = useQueryClient();
  const isAdmin = user?.role === "admin";

  const [range, setRange] = useState<DashboardRange>("1h");
  const [paused, setPaused] = useState(false);
  const [filter, setFilter] = useState<ServiceFilter>("all");
  const [selectedServerId, setSelectedServerId] = useState<string | null>(null);
  const servicesRef = useRef<HTMLDivElement>(null);

  const statsRange = toStatsRange(range);
  const pollFast = paused ? false : LIVE_MS;
  const pollSlow = paused ? false : 30_000;

  const servers = useQuery({
    queryKey: ["servers"],
    queryFn: () => api<Server[]>("/api/v1/servers"),
    refetchInterval: pollFast,
  });
  const services = useQuery({
    queryKey: ["services"],
    queryFn: () => api<Service[]>("/api/v1/services"),
    refetchInterval: pollFast,
  });
  const overview = useQuery({
    queryKey: ["metrics-overview"],
    queryFn: () => api<Schemas["MetricsOverview"]>("/api/v1/metrics/overview"),
    refetchInterval: pollFast,
  });
  const stats = useQuery({
    queryKey: ["statistics", statsRange],
    queryFn: () => api<Schemas["StatsOverview"]>(`/api/v1/stats/overview?range=${statsRange}`),
    refetchInterval: pollSlow,
  });
  const alerts = useQuery({
    queryKey: ["alerts"],
    queryFn: () => api<AlertRule[]>("/api/v1/alerts"),
    refetchInterval: pollSlow,
  });
  const events = useQuery({
    queryKey: ["events", "recent"],
    queryFn: () => api<Event[]>("/api/v1/events?limit=12"),
    refetchInterval: pollSlow,
  });
  const load = useLoad(paused);
  const { pause: pauseBuilds } = useLoadActions();

  const serverList = servers.data ?? [];
  const serviceList = services.data ?? [];
  const alertList = alerts.data ?? [];
  const usage = useMemo(
    () => new Map((overview.data?.servers ?? []).map((row) => [row.server_id, row])),
    [overview.data],
  );
  const serverName = useMemo(
    () => new Map(serverList.map((server) => [server.id, server.name])),
    [serverList],
  );

  const online = serverList.filter((server) => server.status === "online").length;
  const offline = serverList.filter((server) => server.status === "offline");
  const running = serviceList.filter((service) => service.status === "running").length;
  const failing = serviceList.filter((service) => isIssue(service.status));
  const firing = alertList.filter((rule) => rule.enabled && rule.state === "firing");
  const pendingAlerts = alertList.filter((rule) => rule.enabled && rule.state === "pending");
  const loading = servers.isLoading || services.isLoading;

  const refreshing =
    servers.isFetching || services.isFetching || overview.isFetching || stats.isFetching;

  const handleRefresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["servers"] });
    void queryClient.invalidateQueries({ queryKey: ["services"] });
    void queryClient.invalidateQueries({ queryKey: ["metrics-overview"] });
    void queryClient.invalidateQueries({ queryKey: ["metrics"] });
    void queryClient.invalidateQueries({ queryKey: ["statistics", statsRange] });
    void queryClient.invalidateQueries({ queryKey: ["alerts"] });
    void queryClient.invalidateQueries({ queryKey: ["events", "recent"] });
    void queryClient.invalidateQueries({ queryKey: ["load"] });
  };

  const showIssues = () => {
    setFilter("issues");
    servicesRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const issues: DashboardIssue[] = [];
  const view = load.data;
  if (view?.guard.level === "critical") {
    issues.push({
      key: "load",
      tone: "danger",
      icon: "gauge",
      title: t("The panel server is running out of memory"),
      detail: view.guard.reasons.map((reason) => reasonText(t, reason)).join(" · "),
      action: <IssueLink to="/load">{t("Open load controls")}</IssueLink>,
    });
  }
  if (offline.length) {
    issues.push({
      key: "servers",
      tone: "danger",
      icon: "server",
      title:
        offline.length === 1
          ? t("{name} is offline", { name: offline[0]?.name ?? "" })
          : t("{count} servers are offline", { count: offline.length }),
      detail: offline[0]?.last_heartbeat_at
        ? t("Last seen {time}", { time: formatRelative(offline[0].last_heartbeat_at) })
        : undefined,
      action: <IssueLink to="/servers">{t("Servers")}</IssueLink>,
    });
  }
  if (firing.length) {
    issues.push({
      key: "alerts",
      tone: "danger",
      icon: "bell",
      title:
        firing.length === 1
          ? t("{count} alert is firing", { count: firing.length })
          : t("{count} alerts are firing", { count: firing.length }),
      detail: firing.map((rule) => rule.name).join(", "),
      action: <IssueLink to="/alerts">{t("Alerts")}</IssueLink>,
    });
  }
  if (failing.length) {
    issues.push({
      key: "services",
      tone: "danger",
      icon: "rocket",
      title:
        failing.length === 1
          ? t("{count} service needs a look", { count: failing.length })
          : t("{count} services need a look", { count: failing.length }),
      detail: failing
        .slice(0, 3)
        .map((service) => service.name)
        .join(", "),
      action: (
        <button
          type="button"
          className={buttonClass({ variant: "secondary", size: "sm" })}
          onClick={showIssues}
        >
          {t("Show them")}
        </button>
      ),
    });
  }
  if (view?.guard.level === "busy") {
    issues.push({
      key: "load",
      tone: "warning",
      icon: "gauge",
      title: t("The panel server is busy"),
      detail: view.guard.reasons.map((reason) => reasonText(t, reason)).join(" · "),
      action: <IssueLink to="/load">{t("See what uses it")}</IssueLink>,
    });
  }
  if (view?.guard.settings.builds_paused) {
    issues.push({
      key: "paused",
      tone: "warning",
      icon: "pause",
      title: t("Builds are paused"),
      detail: view.builds.queued
        ? t("{count} builds wait until they are resumed.", { count: view.builds.queued })
        : t("New builds wait until they are resumed."),
      action: isAdmin ? (
        <button
          type="button"
          className={buttonClass({ variant: "secondary", size: "sm" })}
          disabled={pauseBuilds.isPending}
          onClick={() => pauseBuilds.mutate(false)}
        >
          <Icon name="play" size={14} />
          {t("Resume builds")}
        </button>
      ) : null,
    });
  }

  const cpuSpark = useMemo(
    () =>
      (stats.data?.metrics?.series ?? [])
        .map((p) => p.cpu_avg)
        .filter((v): v is number => v != null),
    [stats.data],
  );
  const reqSpark = useMemo(
    () =>
      (stats.data?.metrics?.series ?? [])
        .map((p) => p.requests)
        .filter((v): v is number => v != null),
    [stats.data],
  );

  const totals = stats.data?.metrics?.totals;
  const http5xx = totals?.http_5xx ?? 0;
  const http5xxRate = totals?.http_5xx_rate;
  const activeBuilds = view ? view.builds.running + view.builds.stack_builds : 0;
  const buildHold = view ? holdText(t, view.builds.hold) : null;
  const allHealthy = !loading && issues.length === 0 && !services.error && !servers.error;

  return (
    <>
      {/* 1. Quiet, authoritative page header — healthy status folds inline */}
      <div className="flex flex-wrap items-end justify-between gap-x-4 gap-y-3">
        <div className="min-w-0">
          <h1 className="text-[22px] leading-tight font-semibold tracking-[-0.02em] text-ink">
            {t("Overview")}
          </h1>
          <p className="mt-1 flex flex-wrap items-center gap-2 text-[13px] text-muted">
            {allHealthy ? (
              <>
                <span className="inline-flex items-center gap-1.5 font-medium text-pine">
                  <span className="size-1.5 rounded-full bg-pine" aria-hidden="true" />
                  {t("All systems operational")}
                </span>
                <span className="text-faint">·</span>
                <span className="tabular-nums">
                  {t("{online}/{servers} servers online · {running}/{services} services running", {
                    online,
                    servers: serverList.length,
                    running,
                    services: serviceList.length,
                  })}
                </span>
              </>
            ) : (
              <span>{t("Real-time infrastructure health, telemetry and workloads.")}</span>
            )}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <LiveControlBar
            ranges={DASHBOARD_RANGES}
            range={range}
            onRange={setRange}
            paused={paused}
            onTogglePause={() => setPaused((p) => !p)}
            onRefresh={handleRefresh}
            refreshing={refreshing}
            updatedAt={services.dataUpdatedAt}
          />
          {isAdmin ? (
            <div className="flex items-center gap-1.5">
              <Link
                to="/templates"
                className={buttonClass({ variant: "secondary", size: "sm" })}
              >
                {t("Templates")}
              </Link>
              <Link to="/projects" className={buttonClass({ size: "sm" })}>
                <Icon name="plus" size={14} />
                <span>{t("New project")}</span>
              </Link>
            </div>
          ) : null}
        </div>
      </div>

      {/* 2. Incident Triage (only mounts when issues/errors exist) */}
      <AdaptiveHealthStrip
        issues={issues}
        loading={loading}
        error={services.error ?? servers.error}
        updatedAt={services.dataUpdatedAt}
      />

      {/* 3. Unified Swiss Hairline Metric Strip (zero floating icon-box clutter) */}
      <MetricStrip columns={4}>
        <MetricCell
          to="/servers"
          label={t("Servers online")}
          loading={servers.isLoading}
          value={online}
          suffix={`/ ${serverList.length}`}
          tone={offline.length > 0 ? "danger" : "default"}
          visual={
            cpuSpark.length > 1 ? (
              <Sparkline
                values={cpuSpark}
                color={offline.length > 0 ? "var(--chart-4)" : "var(--chart-1)"}
                label={t("Average server CPU")}
              />
            ) : null
          }
          caption={
            offline.length
              ? t("{count} offline", { count: offline.length })
              : totals?.cpu_avg != null
                ? t("Avg CPU {cpu}% · RAM {mem}%", {
                    cpu: totals.cpu_avg.toFixed(0),
                    mem: totals.memory_avg != null ? totals.memory_avg.toFixed(0) : "—",
                  })
                : serverList.length
                  ? t("All reachable")
                  : t("None added yet")
          }
        />

        <MetricCell
          to="/projects"
          label={t("Services running")}
          loading={services.isLoading}
          value={running}
          suffix={`/ ${serviceList.length}`}
          tone={failing.length > 0 ? "danger" : "default"}
          visual={
            serviceList.length > 0 ? (
              <div className="w-16">
                <Meter
                  value={(running / serviceList.length) * 100}
                  label={t("Services running")}
                  className="[&>div]:bg-pine!"
                />
              </div>
            ) : null
          }
          caption={
            failing.length
              ? t("{count} with problems", { count: failing.length })
              : serviceList.length - running
                ? t("{count} stopped", { count: serviceList.length - running })
                : serviceList.length
                  ? t("Everything is up")
                  : t("Nothing deployed yet")
          }
        />

        <MetricCell
          to="/statistics"
          label={t("Proxy requests ({range})", { range: statsRange })}
          loading={stats.isLoading}
          value={totals?.requests != null ? Math.round(totals.requests).toLocaleString() : "—"}
          tone={http5xx > 0 ? "warning" : "default"}
          visual={
            reqSpark.length > 1 ? (
              <Sparkline
                values={reqSpark}
                color={http5xx > 0 ? "var(--chart-2)" : "var(--chart-1)"}
                label={t("Proxy requests")}
              />
            ) : null
          }
          caption={
            http5xx > 0
              ? t("{count} 5xx ({rate}%)", {
                  count: Math.round(http5xx),
                  rate: http5xxRate != null ? (http5xxRate * 100).toFixed(2) : "0.00",
                })
              : stats.data?.operations.deploy_success_rate != null
                ? t("No 5xx · Deploy success {rate}%", {
                    rate: (stats.data.operations.deploy_success_rate * 100).toFixed(0),
                  })
                : t("Zero 5xx errors")
          }
        />

        <MetricCell
          to={firing.length > 0 ? "/alerts" : "/load"}
          label={firing.length > 0 ? t("Alerts firing") : t("Builds & alerts")}
          loading={alerts.isLoading && load.isLoading}
          value={firing.length > 0 ? firing.length : activeBuilds}
          suffix={
            firing.length > 0
              ? t("firing")
              : view?.builds.queued
                ? t("+{count} waiting", { count: view.builds.queued })
                : t("active")
          }
          tone={firing.length > 0 ? "danger" : buildHold ? "warning" : "default"}
          visual={
            <div className="flex h-1.5 w-16 gap-1" aria-hidden="true">
              {Array.from({ length: Math.max(view?.builds.concurrency ?? 1, 1) }, (_, index) => (
                <span
                  key={index}
                  className={`flex-1 rounded-full ${
                    index < activeBuilds ? "bg-pine animate-live" : "bg-sunken"
                  }`}
                />
              ))}
            </div>
          }
          caption={
            firing[0]
              ? firing.length > 1
                ? t("{name} and {count} more", {
                    name: firing[0].name,
                    count: firing.length - 1,
                  })
                : firing[0].name
              : buildHold
                ? buildHold
                : alertList.length
                  ? `${
                      alertList.length === 1
                        ? t("{count} rule", { count: alertList.length })
                        : t("{count} rules", { count: alertList.length })
                    }${
                      pendingAlerts.length
                        ? ` · ${t("{count} pending", { count: pendingAlerts.length })}`
                        : ` · ${t("Queue is empty")}`
                    }`
                  : t("Queue is empty")
          }
        />
      </MetricStrip>

      {/* 4. Balanced Two-Column Operations Grid */}
      <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_350px] xl:grid-cols-[minmax(0,1fr)_380px]">
        <div className="grid min-w-0 gap-5">
          <TelemetryPanel
            servers={serverList}
            selectedServerId={selectedServerId}
            onSelectServer={setSelectedServerId}
            range={range}
            paused={paused}
            stats={stats.data}
            statsLoading={stats.isLoading}
          />
          <div ref={servicesRef} className="min-w-0 scroll-mt-20">
            <UnifiedServicesCard
              services={serviceList}
              usageList={overview.data?.services ?? []}
              servers={serverList}
              loading={services.isLoading}
              isAdmin={isAdmin}
              filter={filter}
              onFilter={setFilter}
            />
          </div>
        </div>
        <div className="grid min-w-0 gap-5">
          <FleetPanel
            servers={serverList}
            usage={usage}
            loading={servers.isLoading}
            selectedServerId={selectedServerId}
            onSelectServer={setSelectedServerId}
          />
          <PanelLoadCard />
          <ActivityFeedCard
            events={events.data ?? []}
            loading={events.isLoading}
            serverName={serverName}
          />
          <ImageStatusCard compact />
        </div>
      </div>
    </>
  );
}

function IssueLink({ to, children }: { to: string; children: ReactNode }) {
  return (
    <Link to={to} className={buttonClass({ variant: "secondary", size: "sm" })}>
      {children}
      <Icon name="arrowRight" size={14} />
    </Link>
  );
}

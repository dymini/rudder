import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { toast } from "sonner";
import { api } from "../../api";
import { errorMessage, formatBytes } from "../../format";
import { isIssue, statusOrder } from "../../health";
import { useT } from "../../i18n";
import type { Schemas } from "../../types";
import { Icon } from "../icons";
import { ThresholdMeter } from "../load";
import {
  Badge,
  Button,
  buttonClass,
  Card,
  Dialog,
  Empty,
  KindTile,
  Segmented,
  Skeleton,
  StatusDot,
} from "../ui";

type Service = Schemas["ServiceSummary"];
type ServiceUsage = Schemas["ServiceUsage"];
type Server = Schemas["ServerView"];

export type ServiceFilter = "all" | "running" | "issues" | "stopped";
type ServiceSort = "priority" | "cpu" | "memory" | "name";
type KindFilter = "all" | "application" | "database" | "compose";

const PAGE = 10;

/**
 * Unified Services Operations Table:
 * Combines service status inventory and Top Consumers live resource usage
 * into a single searchable, filterable, sortable table with quick actions.
 */
export function UnifiedServicesCard({
  services,
  usageList,
  servers,
  loading,
  isAdmin,
  filter,
  onFilter,
}: {
  services: Service[];
  usageList: ServiceUsage[];
  servers: Server[];
  loading: boolean;
  isAdmin: boolean;
  filter: ServiceFilter;
  onFilter: (filter: ServiceFilter) => void;
}) {
  const t = useT();
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<KindFilter>("all");
  const [sort, setSort] = useState<ServiceSort>("priority");
  const [expanded, setExpanded] = useState(false);
  const [stopping, setStopping] = useState<Service | null>(null);

  const serverById = useMemo(() => new Map(servers.map((s) => [s.id, s])), [servers]);
  const usageByServiceId = useMemo(
    () => new Map(usageList.map((u) => [u.service_id, u])),
    [usageList],
  );

  const counts = useMemo(
    () => ({
      all: services.length,
      running: services.filter((s) => s.status === "running").length,
      issues: services.filter((s) => isIssue(s.status)).length,
      stopped: services.filter((s) => s.status === "stopped").length,
    }),
    [services],
  );

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return services
      .filter((service) => {
        if (filter === "running" && service.status !== "running") return false;
        if (filter === "issues" && !isIssue(service.status)) return false;
        if (filter === "stopped" && service.status !== "stopped") return false;
        if (kind !== "all" && service.kind !== kind) return false;
        if (!needle) return true;
        const sName = serverById.get(service.server_id)?.name ?? "";
        return [service.name, service.project_name, service.environment_name, service.kind, sName]
          .join(" ")
          .toLowerCase()
          .includes(needle);
      })
      .sort((a, b) => {
        const ua = usageByServiceId.get(a.id);
        const ub = usageByServiceId.get(b.id);
        if (sort === "cpu") {
          return (ub?.cpu_percent ?? -1) - (ua?.cpu_percent ?? -1) || a.name.localeCompare(b.name);
        }
        if (sort === "memory") {
          return (ub?.memory_bytes ?? -1) - (ua?.memory_bytes ?? -1) || a.name.localeCompare(b.name);
        }
        if (sort === "name") {
          return a.name.localeCompare(b.name);
        }
        return (
          (statusOrder[a.status] ?? 3) - (statusOrder[b.status] ?? 3) ||
          (ub?.cpu_percent ?? 0) - (ua?.cpu_percent ?? 0) ||
          a.project_name.localeCompare(b.project_name) ||
          a.name.localeCompare(b.name)
        );
      });
  }, [services, filter, kind, query, sort, serverById, usageByServiceId]);

  const shown = expanded ? visible : visible.slice(0, PAGE);

  if (!loading && services.length === 0) {
    return (
      <Card title={t("Services")}>
        <Empty
          icon="rocket"
          title={t("Deploy your first service")}
          action={
            isAdmin ? (
              <>
                <Link to="/templates" className={buttonClass({ variant: "secondary", size: "sm" })}>
                  {t("Browse templates")}
                </Link>
                <Link to="/projects" className={buttonClass({ size: "sm" })}>
                  {t("Create a project")}
                </Link>
              </>
            ) : null
          }
        >
          {t("Apps, databases and Compose stacks appear here once a project has them.")}
        </Empty>
      </Card>
    );
  }

  return (
    <>
      <Card
        title={t("Services & workloads")}
        description={t("Live status, CPU and memory across every project.")}
        flush
        actions={
          <div className="flex items-center gap-2">
            <select
              aria-label={t("Filter by kind")}
              value={kind}
              onChange={(e) => setKind(e.target.value as KindFilter)}
              className="h-8 rounded-control border border-line-strong bg-surface px-2 text-xs font-medium text-ink-soft outline-none focus:border-pine"
            >
              <option value="all">{t("All kinds")}</option>
              <option value="application">{t("Applications")}</option>
              <option value="database">{t("Databases")}</option>
              <option value="compose">{t("Compose stacks")}</option>
            </select>
            <select
              aria-label={t("Sort services by")}
              value={sort}
              onChange={(e) => setSort(e.target.value as ServiceSort)}
              className="h-8 rounded-control border border-line-strong bg-surface px-2 text-xs font-medium text-ink-soft outline-none focus:border-pine"
            >
              <option value="priority">{t("Sort: Priority")}</option>
              <option value="cpu">{t("Sort: CPU")}</option>
              <option value="memory">{t("Sort: Memory")}</option>
              <option value="name">{t("Sort: Name")}</option>
            </select>
          </div>
        }
        footer={
          <>
            <span>
              {loading
                ? t("Loading…")
                : visible.length !== services.length
                  ? t("Showing {shown} of {visible} ({total} total)", {
                      shown: shown.length,
                      visible: visible.length,
                      total: services.length,
                    })
                  : t("Showing {shown} of {visible}", {
                      shown: shown.length,
                      visible: visible.length,
                    })}
            </span>
            <span className="flex items-center gap-3">
              {visible.length > PAGE ? (
                <button
                  type="button"
                  className="font-medium text-ink-soft hover:text-ink"
                  onClick={() => setExpanded((value) => !value)}
                >
                  {expanded ? t("Show fewer") : t("Show all {count}", { count: visible.length })}
                </button>
              ) : null}
              <Link to="/projects" className="inline-flex items-center gap-1 font-medium text-pine">
                {t("Projects")} <Icon name="arrowRight" size={14} />
              </Link>
            </span>
          </>
        }
      >
        <div className="flex flex-wrap items-center gap-2 border-b border-line px-5 pb-3.5 sm:px-6">
          <label className="relative min-w-0 flex-1 basis-48">
            <span className="sr-only">{t("Search services")}</span>
            <Icon
              name="search"
              className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-faint"
            />
            <input
              type="search"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("Search by name, project, server or kind")}
              className="h-8 w-full rounded-control border border-line-strong bg-surface pr-3 pl-8 text-sm text-ink outline-none placeholder:text-faint focus:border-pine focus:shadow-[0_0_0_3px_color-mix(in_srgb,var(--pine)_14%,transparent)]"
            />
          </label>
          <div className="max-w-full overflow-x-auto">
            <Segmented<ServiceFilter>
              label={t("Filter services by status")}
              value={filter}
              onChange={onFilter}
              options={[
                ["all", t("All"), counts.all],
                ["running", t("Running"), counts.running],
                ["issues", t("Issues"), counts.issues],
                ["stopped", t("Stopped"), counts.stopped],
              ]}
            />
          </div>
        </div>

        {loading ? (
          <ul className="divide-y divide-line">
            {Array.from({ length: 5 }, (_, index) => (
              <li key={index} className="flex items-center gap-3 px-5 py-3.5 sm:px-6">
                <Skeleton className="size-8 rounded-alert" />
                <div className="flex-1 space-y-1.5">
                  <Skeleton className="h-4 w-40" />
                  <Skeleton className="h-3 w-56 max-w-full" />
                </div>
                <Skeleton className="h-5 w-16 rounded-full" />
              </li>
            ))}
          </ul>
        ) : visible.length === 0 ? (
          <Empty icon="search" title={t("No matching services")}>
            {t("Try another name or clear the filter.")}
          </Empty>
        ) : (
          <ul className="divide-y divide-line">
            {shown.map((service) => {
              const usage = usageByServiceId.get(service.id);
              const server = serverById.get(service.server_id);
              const cpu = usage?.cpu_percent ?? null;
              const memBytes = usage?.memory_bytes ?? null;
              const memShare =
                server?.memory_bytes && memBytes != null
                  ? (memBytes / server.memory_bytes) * 100
                  : null;

              return (
                <li
                  key={service.id}
                  className="group grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-3 gap-y-2 px-5 py-3 transition-colors hover:bg-paper sm:px-6 xl:grid-cols-[auto_minmax(0,1fr)_120px_96px_104px_auto]"
                >
                  <span className="relative">
                    <KindTile kind={service.kind} size={32} />
                    <span className="absolute -right-0.5 -bottom-0.5 inline-flex rounded-full bg-surface p-[2px]">
                      <StatusDot status={service.status} />
                    </span>
                  </span>

                  <Link to={`/services/${service.id}`} className="min-w-0">
                    <span className="block truncate text-sm font-medium text-ink group-hover:text-pine">
                      {service.name}
                    </span>
                    <span className="block truncate text-xs text-muted">
                      {service.project_name}
                      <span className="px-1 text-faint">/</span>
                      {service.environment_name}
                      {server ? (
                        <>
                          <span className="px-1 text-faint">·</span>
                          <span>{server.name}</span>
                        </>
                      ) : null}
                    </span>
                  </Link>

                  {/* Inline CPU meter */}
                  <div className="hidden min-w-0 xl:block">
                    <div className="flex items-baseline justify-between text-[11.5px]">
                      <span className="text-muted">{t("CPU")}</span>
                      <span className="font-medium text-ink-soft tabular-nums">
                        {cpu == null ? "—" : `${cpu.toFixed(cpu < 10 ? 1 : 0)}%`}
                      </span>
                    </div>
                    <div className="mt-1">
                      <ThresholdMeter
                        value={cpu}
                        warn={60}
                        danger={85}
                        marks={false}
                        label={t("CPU usage")}
                      />
                    </div>
                  </div>

                  {/* Inline Memory */}
                  <div className="hidden min-w-0 text-right xl:block">
                    <p className="text-[11.5px] text-muted">{t("Memory")}</p>
                    <p className="text-xs font-medium text-ink-soft tabular-nums">
                      {memBytes == null ? "—" : formatBytes(memBytes)}
                      {memShare != null ? (
                        <span className="ml-1 text-[11px] font-normal text-faint">
                          {Math.round(memShare)}%
                        </span>
                      ) : null}
                    </p>
                  </div>

                  {/* Status Badge */}
                  <div className="hidden justify-end xl:flex">
                    <Badge value={service.status} />
                  </div>

                  {/* Quick Actions */}
                  <div className="flex items-center justify-end gap-1.5">
                    <span className="xl:hidden">
                      <Badge value={service.status} />
                    </span>
                    <Link
                      to="/logs"
                      title={t("Logs")}
                      aria-label={t("Logs")}
                      className="inline-flex size-7 items-center justify-center rounded-control text-muted transition-colors hover:bg-sunken hover:text-ink"
                    >
                      <Icon name="logs" size={14} />
                    </Link>
                    {isAdmin && service.status !== "stopped" ? (
                      <button
                        type="button"
                        title={t("Stop")}
                        aria-label={t("Stop {name}?", { name: service.name })}
                        onClick={() => setStopping(service)}
                        className="inline-flex size-7 items-center justify-center rounded-control text-muted transition-colors hover:bg-danger-soft hover:text-danger"
                      >
                        <Icon name="stop" size={13} />
                      </button>
                    ) : null}
                    <Link
                      to={`/services/${service.id}`}
                      aria-label={t("Open {name}", { name: service.name })}
                      className="inline-flex size-7 items-center justify-center rounded-control text-faint transition-colors hover:bg-sunken hover:text-ink"
                    >
                      <Icon name="chevronRight" size={14} />
                    </Link>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </Card>

      <StopServiceModal service={stopping} onClose={() => setStopping(null)} />
    </>
  );
}

function StopServiceModal({
  service,
  onClose,
}: {
  service: Service | null;
  onClose: () => void;
}) {
  const t = useT();
  const queryClient = useQueryClient();
  const stack = service?.kind === "compose";
  const stop = useMutation({
    mutationFn: (id: string) =>
      api(stack ? `/api/v1/services/${id}/stack/down` : `/api/v1/services/${id}/stop`, {
        method: "POST",
      }),
    onSuccess: () => {
      toast.success(t("{name} is stopping", { name: service?.name ?? "" }));
      void queryClient.invalidateQueries({ queryKey: ["services"] });
      void queryClient.invalidateQueries({ queryKey: ["metrics-overview"] });
      onClose();
    },
    onError: (error) => toast.error(errorMessage(error, t("Could not stop the service"))),
  });

  return (
    <Dialog
      open={service != null}
      onClose={onClose}
      title={t("Stop {name}?", { name: service?.name ?? "" })}
      description={
        stack
          ? t(
              "The stack's containers are removed with docker compose down; volumes and data are kept. Start it again from its page.",
            )
          : t(
              "Its container stops and visitors get errors until you start it again from its page. Data is kept.",
            )
      }
    >
      <div className="flex flex-wrap justify-end gap-2">
        <Button quiet onClick={onClose}>
          {t("Keep it running")}
        </Button>
        <Button
          variant="danger-solid"
          icon="stop"
          disabled={!service || stop.isPending}
          onClick={() => service && stop.mutate(service.id)}
        >
          {stack ? t("Stop the stack") : t("Stop the service")}
        </Button>
      </div>
    </Dialog>
  );
}

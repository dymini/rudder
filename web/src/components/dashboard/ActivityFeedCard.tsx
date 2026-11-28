import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { formatRelative, formatTime } from "../../format";
import { useT } from "../../i18n";
import type { Schemas } from "../../types";
import { Icon, type IconName } from "../icons";
import { buttonClass, Card, Empty, Segmented, Skeleton } from "../ui";

type Event = Schemas["EventView"];
type ActivityFilter = "all" | "alerts" | "deploys";

const infoStyle: [string, IconName] = ["border-line bg-sunken text-ink-soft", "info"];

const severityStyle: Record<string, [string, IconName]> = {
  critical: ["border-danger-line bg-danger-soft text-danger", "alert"],
  error: ["border-danger-line bg-danger-soft text-danger", "alert"],
  warning: ["border-amber-line bg-amber-soft text-amber", "alert"],
  info: ["border-line bg-sunken text-ink-soft", "info"],
};

function matchesFilter(event: Event, filter: ActivityFilter): boolean {
  if (filter === "all") return true;
  if (filter === "alerts") {
    return (
      event.severity === "critical" ||
      event.severity === "error" ||
      event.severity === "warning" ||
      event.kind.startsWith("alert.") ||
      event.kind.startsWith("guard.")
    );
  }
  return event.kind.startsWith("deploy") || event.kind.startsWith("build") || event.kind.startsWith("service.");
}

/** Recent activity timeline with quick filter chips (All / Alerts / Deploys). */
export function ActivityFeedCard({
  events,
  loading,
  serverName,
}: {
  events: Event[];
  loading: boolean;
  serverName: Map<string, string>;
}) {
  const t = useT();
  const [filter, setFilter] = useState<ActivityFilter>("all");

  const filtered = useMemo(
    () => events.filter((event) => matchesFilter(event, filter)),
    [events, filter],
  );

  return (
    <Card
      title={t("Recent activity")}
      flush
      actions={
        <Link to="/logs" className={buttonClass({ variant: "ghost", size: "sm" })}>
          {t("Logs")}
          <Icon name="arrowRight" size={14} />
        </Link>
      }
    >
      <div className="border-b border-line px-4 pb-3 sm:px-5">
        <Segmented<ActivityFilter>
          label={t("Filter activity")}
          value={filter}
          onChange={setFilter}
          options={[
            ["all", t("All")],
            ["alerts", t("Alerts")],
            ["deploys", t("Deploys")],
          ]}
        />
      </div>

      {loading ? (
        <div className="space-y-3 px-4 py-4 sm:px-5">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : filtered.length === 0 ? (
        <Empty icon="history" title={t("All quiet")}>
          {filter === "all"
            ? t("Deploys, backups and alerts will show up here.")
            : t("No matching events in the recent feed.")}
        </Empty>
      ) : (
        <ol className="px-4 py-2 sm:px-5">
          {filtered.map((event, index) => {
            const [cls, icon] = event.kind.startsWith("guard.")
              ? ([
                  (severityStyle[event.severity] ?? infoStyle)[0],
                  "gauge",
                ] as [string, IconName])
              : (severityStyle[event.severity] ?? infoStyle);

            return (
              <li key={event.id} className="relative flex gap-3 py-2.5">
                {index < filtered.length - 1 ? (
                  <span
                    className="absolute top-9 bottom-0 left-[13px] w-px bg-line"
                    aria-hidden="true"
                  />
                ) : null}
                <span
                  className={`relative inline-flex size-7 shrink-0 items-center justify-center rounded-full border ${cls}`}
                >
                  <Icon name={icon} size={13} />
                </span>
                <div className="min-w-0 flex-1">
                  <p className="flex items-baseline justify-between gap-2">
                    <span className="truncate text-[13.5px] font-medium text-ink">
                      {event.title}
                    </span>
                    <time
                      dateTime={event.created_at}
                      title={formatTime(event.created_at)}
                      className="shrink-0 text-[11.5px] text-faint tabular-nums"
                    >
                      {formatRelative(event.created_at)}
                    </time>
                  </p>
                  <p className="line-clamp-2 text-xs text-muted">
                    {event.server_id && serverName.get(event.server_id) ? (
                      <span className="text-ink-soft">{serverName.get(event.server_id)} · </span>
                    ) : null}
                    {event.message}
                  </p>
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </Card>
  );
}

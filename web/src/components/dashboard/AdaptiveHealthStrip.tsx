import type { ReactNode } from "react";
import { errorMessage, formatRelative, formatTime } from "../../format";
import { useT } from "../../i18n";
import { Icon, type IconName } from "../icons";

export interface DashboardIssue {
  key: string;
  tone: "danger" | "warning" | "info";
  icon: IconName;
  title: ReactNode;
  detail?: ReactNode;
  action?: ReactNode;
}

const issueTone: Record<DashboardIssue["tone"], string> = {
  danger: "border-danger-line bg-danger-soft text-danger",
  warning: "border-amber-line bg-amber-soft text-amber",
  info: "border-line bg-sunken text-ink-soft",
};

/**
 * Adaptive system health strip:
 * - Compact single-row operational banner when everything is healthy.
 * - Expands into a prioritized, actionable incident triage list when issues exist.
 */
export function AdaptiveHealthStrip({
  issues,
  loading,
  error,
  updatedAt,
  onlineServers,
  totalServers,
  runningServices,
  totalServices,
}: {
  issues: DashboardIssue[];
  loading: boolean;
  error: unknown;
  updatedAt: number;
  onlineServers: number;
  totalServers: number;
  runningServices: number;
  totalServices: number;
}) {
  const t = useT();

  if (error) {
    return (
      <section className="flex items-start gap-3.5 rounded-card border border-danger-line bg-danger-soft px-4 py-3.5 text-danger sm:px-5">
        <Icon name="alert" size={20} className="mt-0.5 shrink-0" />
        <div>
          <h2 className="text-sm font-semibold">{t("Couldn't load the overview")}</h2>
          <p className="mt-0.5 text-[13px]">
            {errorMessage(error, t("The panel did not answer."))}
          </p>
        </div>
      </section>
    );
  }

  const danger = issues.some((issue) => issue.tone === "danger");
  const calm = !loading && issues.length === 0;

  if (calm) {
    return (
      <section className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 rounded-card border border-pine-line/80 bg-surface px-4 py-2.5 shadow-card sm:px-5">
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="inline-flex size-6 shrink-0 items-center justify-center rounded-full bg-pine-soft text-pine">
            <Icon name="check" size={14} />
          </span>
          <p className="truncate text-sm font-medium text-ink">
            {t("All systems operational")}
          </p>
          <span className="hidden text-xs text-muted sm:inline">·</span>
          <p className="hidden truncate text-xs text-muted tabular-nums sm:inline">
            {t("{online}/{servers} servers online · {running}/{services} services running", {
              online: onlineServers,
              servers: totalServers,
              running: runningServices,
              services: totalServices,
            })}
          </p>
        </div>
        {updatedAt ? (
          <span
            className="shrink-0 text-xs text-faint tabular-nums"
            title={formatTime(new Date(updatedAt).toISOString())}
          >
            {t("Updated {time}", { time: formatRelative(new Date(updatedAt).toISOString()) })}
          </span>
        ) : null}
      </section>
    );
  }

  return (
    <section
      className={`overflow-hidden rounded-card border bg-surface shadow-card ${
        danger ? "border-danger-line" : loading ? "border-line" : "border-amber-line"
      }`}
    >
      <div className="flex flex-wrap items-center gap-3 px-4 py-3 sm:px-5">
        <span
          className={`inline-flex size-8 shrink-0 items-center justify-center rounded-alert border ${
            loading
              ? "border-line bg-sunken text-faint"
              : danger
                ? "border-danger-line bg-danger-soft text-danger"
                : "border-amber-line bg-amber-soft text-amber"
          }`}
        >
          <Icon name={loading ? "activity" : "alert"} size={16} />
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="text-sm leading-snug font-semibold text-ink">
            {loading
              ? t("Checking your servers…")
              : issues.length === 1
                ? t("One thing needs attention")
                : t("{count} things need attention", { count: issues.length })}
          </h2>
          {!loading ? (
            <p className="text-xs text-muted">
              {t("Prioritized by impact. Resolve from the top row.")}
            </p>
          ) : null}
        </div>
        {updatedAt ? (
          <span
            className="text-xs text-faint tabular-nums"
            title={formatTime(new Date(updatedAt).toISOString())}
          >
            {t("Updated {time}", { time: formatRelative(new Date(updatedAt).toISOString()) })}
          </span>
        ) : null}
      </div>

      {issues.length > 0 ? (
        <ul className="divide-y divide-line border-t border-line">
          {issues.map((issue) => (
            <li
              key={issue.key}
              className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2.5 sm:px-5"
            >
              <span
                className={`inline-flex size-7 shrink-0 items-center justify-center rounded-alert border ${issueTone[issue.tone]}`}
              >
                <Icon name={issue.icon} size={15} />
              </span>
              <div className="min-w-0 flex-1 basis-52">
                <p className="text-sm font-medium text-ink">{issue.title}</p>
                {issue.detail ? (
                  <p className="truncate text-xs text-muted">{issue.detail}</p>
                ) : null}
              </div>
              {issue.action ? <div className="shrink-0">{issue.action}</div> : null}
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}

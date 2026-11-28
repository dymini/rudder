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

const dotTone: Record<DashboardIssue["tone"], string> = {
  danger: "bg-danger",
  warning: "bg-amber",
  info: "bg-faint",
};

/**
 * Incident triage panel — renders ONLY when there is an active error or issue.
 * When all systems are healthy, zero vertical chrome is wasted (healthy status lives in the page header).
 */
export function AdaptiveHealthStrip({
  issues,
  loading,
  error,
  updatedAt,
}: {
  issues: DashboardIssue[];
  loading: boolean;
  error: unknown;
  updatedAt: number;
}) {
  const t = useT();

  if (error) {
    return (
      <section className="flex items-start gap-3 rounded-card border border-danger-line border-l-[3px] border-l-danger bg-danger-soft/80 px-4 py-3 text-danger sm:px-5">
        <Icon name="alert" size={18} className="mt-0.5 shrink-0" />
        <div className="min-w-0">
          <h2 className="text-sm font-semibold">{t("Couldn't load the overview")}</h2>
          <p className="mt-0.5 text-[13px]">
            {errorMessage(error, t("The panel did not answer."))}
          </p>
        </div>
      </section>
    );
  }

  if (loading || issues.length === 0) {
    return null;
  }

  const danger = issues.some((issue) => issue.tone === "danger");

  return (
    <section
      className={`overflow-hidden rounded-card border border-l-[3px] bg-surface ${
        danger ? "border-danger-line border-l-danger" : "border-amber-line border-l-amber"
      }`}
    >
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-line px-4 py-2.5 sm:px-5">
        <div className="flex items-center gap-2">
          <span
            className={`size-2 rounded-full ${danger ? "bg-danger animate-live" : "bg-amber"}`}
            aria-hidden="true"
          />
          <h2 className="text-[13px] font-semibold text-ink">
            {issues.length === 1
              ? t("One thing needs attention")
              : t("{count} things need attention", { count: issues.length })}
          </h2>
          <span className="hidden text-xs text-muted sm:inline">
            · {t("Prioritized by impact. Resolve from the top row.")}
          </span>
        </div>
        {updatedAt ? (
          <span
            className="font-mono text-[11px] text-faint tabular-nums"
            title={formatTime(new Date(updatedAt).toISOString())}
          >
            {formatRelative(new Date(updatedAt).toISOString())}
          </span>
        ) : null}
      </div>

      <ul className="divide-y divide-line">
        {issues.map((issue) => (
          <li
            key={issue.key}
            className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2.5 sm:px-5"
          >
            <span className={`size-2 shrink-0 rounded-full ${dotTone[issue.tone]}`} aria-hidden="true" />
            <div className="min-w-0 flex-1 basis-52">
              <p className="text-[13.5px] font-medium text-ink">{issue.title}</p>
              {issue.detail ? (
                <p className="truncate text-xs text-muted">{issue.detail}</p>
              ) : null}
            </div>
            {issue.action ? <div className="shrink-0">{issue.action}</div> : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

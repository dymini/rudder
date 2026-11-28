import { useId, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { formatRelative, formatTime } from "../format";
import { useT } from "../i18n";
import { Icon, type IconName } from "./icons";
import { Segmented, Skeleton } from "./ui";

export type KpiTone = "default" | "pine" | "warning" | "danger";

const toneIconStyle: Record<KpiTone, string> = {
  default: "bg-sunken text-ink-soft",
  pine: "bg-pine-soft text-pine",
  warning: "bg-amber-soft text-amber",
  danger: "bg-danger-soft text-danger",
};

const toneBorderStyle: Record<KpiTone, string> = {
  default: "border-line",
  pine: "border-pine-line/80",
  warning: "border-amber-line",
  danger: "border-danger-line",
};

/** Compact, production-grade Bento KPI card used across Dashboard and Observe pages. */
export function KpiCard({
  to,
  icon,
  label,
  value,
  suffix,
  caption,
  loading = false,
  tone = "default",
  visual,
  badge,
}: {
  to?: string;
  icon?: IconName;
  label: string;
  value: ReactNode;
  suffix?: ReactNode;
  caption?: ReactNode;
  loading?: boolean;
  tone?: KpiTone;
  visual?: ReactNode;
  badge?: ReactNode;
}) {
  const inner = (
    <>
      <div className="flex items-center justify-between gap-2 text-[13px] font-medium text-muted">
        <span className="flex min-w-0 items-center gap-2">
          {icon ? (
            <span
              className={`inline-flex size-7 shrink-0 items-center justify-center rounded-alert ${toneIconStyle[tone]}`}
            >
              <Icon name={icon} size={15} />
            </span>
          ) : null}
          <span className="truncate">{label}</span>
        </span>
        {badge ? (
          <span className="shrink-0">{badge}</span>
        ) : to ? (
          <Icon
            name="arrowRight"
            size={14}
            className="shrink-0 -translate-x-1 text-faint opacity-0 transition group-hover:translate-x-0 group-hover:opacity-100"
          />
        ) : null}
      </div>

      <div className="mt-3.5 flex items-end justify-between gap-3">
        <div className="min-w-0">
          {loading ? (
            <Skeleton className="h-7 w-20" />
          ) : (
            <p className="flex items-baseline gap-1.5 tabular-nums">
              <span className="text-[26px] leading-none font-semibold tracking-[-0.02em] text-ink">
                {value}
              </span>
              {suffix ? <span className="text-xs font-medium text-muted">{suffix}</span> : null}
            </p>
          )}
        </div>
        {visual ? <div className="shrink-0">{visual}</div> : null}
      </div>

      {caption != null ? (
        <div
          className={`mt-2.5 flex items-center gap-1.5 truncate text-[12.5px] ${
            tone === "danger"
              ? "font-medium text-danger"
              : tone === "warning"
                ? "font-medium text-amber"
                : "text-muted"
          }`}
        >
          {tone === "danger" || tone === "warning" ? (
            <Icon name="alert" size={13} className="shrink-0" />
          ) : null}
          <span className="truncate">{loading ? " " : caption}</span>
        </div>
      ) : null}
    </>
  );

  const baseClass = `group flex min-w-0 flex-col justify-between rounded-card border bg-surface p-4 shadow-card transition-[border-color,box-shadow,background-color] sm:p-4.5 ${toneBorderStyle[tone]}`;

  if (to) {
    return (
      <Link
        to={to}
        className={`${baseClass} hover:border-line-strong hover:bg-surface-raised hover:shadow-raised`}
      >
        {inner}
      </Link>
    );
  }

  return <div className={baseClass}>{inner}</div>;
}

/** Lightweight SVG sparkline with soft gradient fill — smooth and jitter-free under live polling. */
export function Sparkline({
  values,
  color = "var(--chart-1)",
  width = 88,
  height = 28,
  label,
}: {
  values: number[];
  color?: string;
  width?: number;
  height?: number;
  label?: string;
}) {
  const gradId = useId();
  const clean = values.filter((v) => Number.isFinite(v));
  if (clean.length < 2) {
    return (
      <svg
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        aria-label={label}
        aria-hidden={!label}
        className="overflow-visible"
      >
        <line
          x1={2}
          y1={height - 6}
          x2={width - 2}
          y2={height - 6}
          stroke="var(--line-strong)"
          strokeWidth={1.5}
          strokeDasharray="2 3"
        />
      </svg>
    );
  }

  const min = Math.min(...clean);
  const max = Math.max(...clean);
  const span = max - min || 1;
  const padY = 3;
  const plotH = height - padY * 2;
  const stepX = (width - 4) / (clean.length - 1);

  const coords = clean.map((v, i) => {
    const x = 2 + i * stepX;
    const y = padY + plotH - ((v - min) / span) * plotH;
    return [x, y] as const;
  });

  const linePath = coords
    .map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`)
    .join(" ");
  const areaPath = `${linePath} L${coords[coords.length - 1]![0].toFixed(1)},${height} L${coords[0]![0].toFixed(1)},${height} Z`;
  const [lastX, lastY] = coords[coords.length - 1]!;

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      aria-label={label}
      aria-hidden={!label}
      className="overflow-visible"
    >
      <defs>
        <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity={0.28} />
          <stop offset="100%" stopColor={color} stopOpacity={0} />
        </linearGradient>
      </defs>
      <path d={areaPath} fill={`url(#${gradId})`} />
      <path
        d={linePath}
        fill="none"
        stroke={color}
        strokeWidth={1.75}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx={lastX} cy={lastY} r={2.2} fill={color} />
    </svg>
  );
}

/** Shared live-sync control bar: time range selector + live/paused polling toggle + manual refresh. */
export function LiveControlBar<R extends string>({
  ranges,
  range,
  onRange,
  paused,
  onTogglePause,
  onRefresh,
  refreshing = false,
  updatedAt,
}: {
  ranges?: readonly R[];
  range?: R;
  onRange?: (value: R) => void;
  paused: boolean;
  onTogglePause: () => void;
  onRefresh: () => void;
  refreshing?: boolean;
  updatedAt?: number;
}) {
  const t = useT();
  return (
    <div className="flex flex-wrap items-center gap-2">
      {ranges && range && onRange ? (
        <Segmented<R>
          label={t("Time range")}
          value={range}
          onChange={onRange}
          options={ranges.map((r) => [r, r] as const)}
        />
      ) : null}

      <div className="inline-flex items-center rounded-control border border-line bg-surface p-0.5 shadow-card">
        <button
          type="button"
          onClick={onTogglePause}
          aria-pressed={!paused}
          title={paused ? t("Resume live updates") : t("Pause live updates")}
          className="inline-flex h-7 items-center gap-1.5 rounded-[4px] px-2 text-xs font-medium text-ink-soft transition-colors hover:bg-sunken hover:text-ink"
        >
          <span
            className={`size-2 rounded-full ${
              paused ? "bg-faint" : "bg-pine animate-live"
            }`}
            aria-hidden="true"
          />
          <span>{paused ? t("Paused") : t("Live")}</span>
        </button>
        <span className="h-4 w-px bg-line" aria-hidden="true" />
        <button
          type="button"
          onClick={onRefresh}
          disabled={refreshing}
          aria-label={t("Refresh now")}
          title={
            updatedAt
              ? `${t("Refresh now")} · ${formatTime(new Date(updatedAt).toISOString())}`
              : t("Refresh now")
          }
          className="inline-flex h-7 items-center gap-1 rounded-[4px] px-2 text-xs font-medium text-muted transition-colors hover:bg-sunken hover:text-ink disabled:opacity-55"
        >
          <Icon
            name="refresh"
            size={13}
            className={refreshing ? "animate-spin text-pine" : ""}
          />
          {updatedAt ? (
            <span className="hidden tabular-nums sm:inline">
              {formatRelative(new Date(updatedAt).toISOString())}
            </span>
          ) : null}
        </button>
      </div>
    </div>
  );
}

/** Crisp section heading for grouping Bento rows on Observe & Statistics pages. */
export function SectionHeading({
  title,
  description,
  actions,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-2">
      <div className="min-w-0">
        <h2 className="text-[13px] font-semibold tracking-[0.02em] text-ink-soft uppercase">
          {title}
        </h2>
        {description ? <p className="mt-0.5 text-xs text-muted">{description}</p> : null}
      </div>
      {actions ? <div className="shrink-0">{actions}</div> : null}
    </div>
  );
}

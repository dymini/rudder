import { useId, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { formatRelative, formatTime } from "../format";
import { useT } from "../i18n";
import { Icon } from "./icons";
import { Segmented, Skeleton } from "./ui";

export type MetricTone = "default" | "pine" | "warning" | "danger";

/**
 * Cohesive hairline-divided Swiss metric strip.
 * Replaces repetitive floating icon-box cards with one unified instrument bar.
 */
export function MetricStrip({
  columns = 4,
  children,
}: {
  columns?: 2 | 3 | 4 | 5;
  children: ReactNode;
}) {
  const gridCols: Record<2 | 3 | 4 | 5, string> = {
    2: "grid-cols-1 sm:grid-cols-2",
    3: "grid-cols-1 sm:grid-cols-3",
    4: "grid-cols-2 lg:grid-cols-4",
    5: "grid-cols-2 md:grid-cols-3 lg:grid-cols-5",
  };
  return (
    <div className="overflow-hidden rounded-card border border-line bg-line">
      <div className={`grid gap-px ${gridCols[columns]}`}>{children}</div>
    </div>
  );
}

/**
 * Single instrument cell inside a MetricStrip.
 * Typography, tabular numerals, and micro-telemetry carry the hierarchy — no decorative icon badges.
 */
export function MetricCell({
  to,
  label,
  value,
  suffix,
  caption,
  loading = false,
  tone = "default",
  visual,
}: {
  to?: string;
  label: string;
  value: ReactNode;
  suffix?: ReactNode;
  caption?: ReactNode;
  loading?: boolean;
  tone?: MetricTone;
  visual?: ReactNode;
}) {
  const accentColor =
    tone === "danger"
      ? "text-danger"
      : tone === "warning"
        ? "text-amber"
        : tone === "pine"
          ? "text-pine"
          : "text-muted";

  const content = (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-[12px] font-medium tracking-[0.005em] text-muted">
          {label}
        </span>
        {to ? (
          <Icon
            name="arrowRight"
            size={13}
            className="shrink-0 -translate-x-1 text-faint opacity-0 transition group-hover:translate-x-0 group-hover:opacity-100"
          />
        ) : null}
      </div>

      <div className="mt-2.5 flex items-end justify-between gap-3">
        <div className="min-w-0">
          {loading ? (
            <Skeleton className="h-7 w-20" />
          ) : (
            <p className="flex items-baseline gap-1.5 tabular-nums">
              <span
                className={`text-[26px] leading-none font-semibold tracking-[-0.03em] ${
                  tone === "danger" ? "text-danger" : "text-ink"
                }`}
              >
                {value}
              </span>
              {suffix ? (
                <span className="text-xs font-normal text-muted">{suffix}</span>
              ) : null}
            </p>
          )}
        </div>
        {visual ? <div className="shrink-0">{visual}</div> : null}
      </div>

      {caption != null ? (
        <p className={`mt-2 truncate text-[12px] tabular-nums ${accentColor}`}>
          {loading ? " " : caption}
        </p>
      ) : null}
    </>
  );

  const cellClass =
    "group flex min-w-0 flex-col justify-between bg-surface px-4 py-3.5 transition-colors sm:px-5 sm:py-4";

  if (to) {
    return (
      <Link to={to} className={`${cellClass} hover:bg-paper`}>
        {content}
      </Link>
    );
  }
  return <div className={cellClass}>{content}</div>;
}

/** Precision SVG sparkline with clean thin stroke and subtle gradient tint. */
export function Sparkline({
  values,
  color = "var(--chart-1)",
  width = 84,
  height = 26,
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
          y1={height - 5}
          x2={width - 2}
          y2={height - 5}
          stroke="var(--line-strong)"
          strokeWidth={1.25}
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
          <stop offset="0%" stopColor={color} stopOpacity={0.2} />
          <stop offset="100%" stopColor={color} stopOpacity={0} />
        </linearGradient>
      </defs>
      <path d={areaPath} fill={`url(#${gradId})`} />
      <path
        d={linePath}
        fill="none"
        stroke={color}
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <circle cx={lastX} cy={lastY} r={2} fill={color} />
    </svg>
  );
}

/**
 * Quiet, precision live-sync bar: time-range selector + pause/resume + manual refresh.
 * Avoids repeating "Live" text that already lives in the global topbar.
 */
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

      <div className="inline-flex h-8 items-center rounded-control border border-line bg-surface p-0.5">
        <button
          type="button"
          onClick={onTogglePause}
          aria-pressed={!paused}
          aria-label={paused ? t("Resume live updates") : t("Pause live updates")}
          title={paused ? t("Resume live updates") : t("Pause live updates")}
          className="inline-flex h-full items-center gap-1.5 rounded-[4px] px-2 text-xs font-medium text-ink-soft transition-colors hover:bg-sunken hover:text-ink"
        >
          <Icon name={paused ? "play" : "pause"} size={12} className={paused ? "text-amber" : "text-pine"} />
          {paused ? <span className="text-amber">{t("Paused")}</span> : null}
        </button>
        <span className="h-3.5 w-px bg-line" aria-hidden="true" />
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
          className="inline-flex h-full items-center gap-1.5 rounded-[4px] px-2 text-xs text-muted transition-colors hover:bg-sunken hover:text-ink disabled:opacity-55"
        >
          <Icon name="refresh" size={13} className={refreshing ? "animate-spin text-pine" : ""} />
          {updatedAt ? (
            <span className="hidden font-mono text-[11px] tabular-nums sm:inline">
              {formatRelative(new Date(updatedAt).toISOString())}
            </span>
          ) : null}
        </button>
      </div>
    </div>
  );
}

/** Architectural section divider heading for Statistics & Observe groups. */
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
    <div className="flex flex-wrap items-baseline justify-between gap-2">
      <div className="min-w-0">
        <h2 className="text-[11.5px] font-semibold tracking-[0.06em] text-muted uppercase">
          {title}
        </h2>
        {description ? <p className="mt-0.5 text-xs text-faint">{description}</p> : null}
      </div>
      {actions ? <div className="shrink-0">{actions}</div> : null}
    </div>
  );
}

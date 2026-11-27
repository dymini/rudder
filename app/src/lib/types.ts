import type { components } from "./schema";

export type Schemas = components["schemas"];
export type ServiceSummary = Schemas["ServiceSummary"];
export type ServiceView = Schemas["ServiceView"];
export type SystemInfo = Schemas["SystemInfo"];
export type ProjectView = Schemas["ProjectView"];
export type EnvironmentView = Schemas["EnvironmentView"];
export type BackupJobView = Schemas["BackupJobView"];
export type BackupRunView = Schemas["BackupRunView"];
export type DomainView = Schemas["DomainView"];
export type DeploymentView = Schemas["DeploymentView"];
export type UpdateOverview = Schemas["UpdateOverview"];
export type AlertView = Schemas["AlertView"];
export type MetricsOverview = Schemas["MetricsOverview"];
export type ServerView = Schemas["ServerView"];
export type EventView = Schemas["EventView"];
export type StackView = Schemas["StackView"];
export type EnvVarView = Schemas["EnvVarView"];
export type LogsView = Schemas["LogsView"];
export type UserView = Schemas["UserView"];
export type McpGrantView = Schemas["McpGrantView"];
export type TokenView = Schemas["TokenView"];
export type DatabaseEngineView = Schemas["DatabaseEngineView"];
export type ServiceUsage = Schemas["ServiceUsage"];
export type LoadView = Schemas["LoadView"];

export function bytes(value: number | null | undefined): string {
  if (value == null) return "—";
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = value;
  let unit = "B";
  for (const next of units) {
    if (v < 1024) break;
    v /= 1024;
    unit = next;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${unit}`;
}

export function timeAgo(iso: string | null | undefined): string {
  if (!iso) return "—";
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const secs = Math.max(0, Math.floor((Date.now() - then) / 1000));
  if (secs < 60) return `${secs}s trước`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m trước`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h trước`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d trước`;
  return new Date(iso).toLocaleDateString();
}

export function formatTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString();
}

import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { api } from "../api";
import { formatBytes, formatRelative } from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";
import { Icon } from "./icons";
import { Badge, Card, Skeleton } from "./ui";

type Overview = Schemas["ImageStoreOverview"];

/** Dashboard card: is Dymini Panel Image set up, and how much is it used. */
export function ImageStatusCard({ compact = false }: { compact?: boolean }) {
  const t = useT();
  const overview = useQuery({
    queryKey: ["image-store-overview"],
    queryFn: () => api<Overview>("/api/v1/image-store/overview"),
    refetchInterval: 60_000,
  });
  const data = overview.data;
  const pending = (data?.setup ?? []).filter((step) => step.state === "todo").length;
  const ready = data ? pending === 0 : false;
  const lastPush = data?.recent_pushes[0];

  // When setup is complete and compact mode is enabled, render a single sleek status row.
  if (compact && ready && data) {
    return (
      <Link
        to="/dymini-panel-image"
        className="group flex items-center justify-between gap-3 rounded-card border border-line bg-surface px-4 py-3 shadow-card transition-[border-color,box-shadow] hover:border-line-strong hover:shadow-raised"
      >
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="inline-flex size-8 shrink-0 items-center justify-center rounded-alert bg-pine-soft text-pine">
            <Icon name="box" size={15} />
          </span>
          <div className="min-w-0">
            <p className="flex items-center gap-2 text-sm font-medium text-ink">
              <span className="truncate">{t("Dymini Panel Image")}</span>
              <Badge value={t("ready")} tone="success" />
            </p>
            <p className="truncate text-xs text-muted tabular-nums">
              {data.destination ? `${data.destination.name} · ` : ""}
              {data.totals.r2_images} {t("versions")} ({formatBytes(data.totals.r2_bytes)})
              {lastPush ? ` · ${formatRelative(lastPush.created_at)}` : ""}
            </p>
          </div>
        </div>
        <Icon
          name="chevronRight"
          size={15}
          className="shrink-0 text-faint transition-transform group-hover:translate-x-0.5 group-hover:text-ink"
        />
      </Link>
    );
  }

  return (
    <Card
      title={t("Dymini Panel Image")}
      actions={
        <Link to="/dymini-panel-image" className="text-sm text-muted hover:text-ink hover:underline">
          {t("Open")}
        </Link>
      }
    >
      {overview.isLoading ? (
        <Skeleton className="h-16 w-full" />
      ) : !data ? (
        <p className="text-sm text-muted">{t("Could not load Dymini Panel Image")}</p>
      ) : (
        <dl className="space-y-2.5 text-sm">
          <div className="flex items-center justify-between gap-3">
            <dt className="text-muted">{t("Bucket")}</dt>
            <dd className="min-w-0 truncate">
              {data.destination ? (
                <span title={data.destination.bucket}>{data.destination.name}</span>
              ) : (
                <Badge value={t("not set")} tone="warning" />
              )}
            </dd>
          </div>
          <div className="flex items-center justify-between gap-3">
            <dt className="text-muted">{t("CLI client")}</dt>
            <dd>
              <Badge
                value={
                  data.clients.length > 0
                    ? t("{count} signed in", { count: data.clients.length })
                    : t("none yet")
                }
                tone={data.clients.length > 0 ? "success" : "outline"}
              />
            </dd>
          </div>
          <div className="flex items-center justify-between gap-3">
            <dt className="text-muted">{t("Versions on R2")}</dt>
            <dd className="tabular-nums">
              {data.totals.r2_images} · {formatBytes(data.totals.r2_bytes)}
            </dd>
          </div>
          <div className="flex items-center justify-between gap-3">
            <dt className="text-muted">{t("Pushes, 7 days")}</dt>
            <dd className="tabular-nums">{data.totals.pushes_7d}</dd>
          </div>
          <div className="flex items-center justify-between gap-3">
            <dt className="text-muted">{t("Last push")}</dt>
            <dd className="text-muted">{lastPush ? formatRelative(lastPush.created_at) : "—"}</dd>
          </div>
          <div className="flex items-center justify-between gap-3 border-t border-line pt-2.5">
            <dt className="text-muted">{t("Setup")}</dt>
            <dd>
              <Badge
                value={ready ? t("ready") : t("{count} steps left", { count: pending })}
                tone={ready ? "success" : "warning"}
              />
            </dd>
          </div>
        </dl>
      )}
    </Card>
  );
}

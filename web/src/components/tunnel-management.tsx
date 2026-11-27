import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { toast } from "sonner";
import { api } from "../api";
import { useT } from "../i18n";
import type { Schemas } from "../types";
import { Alert, Badge, Button } from "./ui";

type Status = Schemas["TunnelStatus"];
const KEY = ["cloudflare", "tunnel-management"];

export function TunnelManagement({
  onRunningChange,
}: {
  onRunningChange: (running: boolean) => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const status = useQuery({
    queryKey: KEY,
    queryFn: () => api<Status>("/api/v1/cloudflare/tunnel/status"),
    refetchInterval: (query) =>
      query.state.data?.update?.status === "running" ? 2000 : 30000,
  });
  const data = status.data;
  const phase = data?.update?.status;
  const busy = phase === "running";
  useEffect(() => {
    onRunningChange(busy);
    if (phase && phase !== "running")
      void client.invalidateQueries({ queryKey: ["cloudflare"], exact: true });
  }, [busy, phase, onRunningChange, client]);
  const check = useMutation({
    mutationFn: () =>
      api<Status>("/api/v1/cloudflare/tunnel/check", { method: "POST" }),
    onSuccess: (data) => client.setQueryData(KEY, data),
    onError: (error) => toast.error(error.message),
  });
  const update = useMutation({
    mutationFn: () =>
      api<Schemas["TunnelUpdateProgress"]>("/api/v1/cloudflare/tunnel/update", {
        method: "POST",
        body: "{}",
      }),
    onSuccess: (progress) =>
      client.setQueryData<Status>(KEY, (data) =>
        data ? { ...data, update: progress } : data,
      ),
    onError: (error) => toast.error(error.message),
  });
  if (status.isPending) return <p role="status">{t("Loading connectors…")}</p>;
  if (!data)
    return (
      <Alert tone="warning" title={t("Could not load tunnel management")}>
        {status.error?.message}
        <Button quiet onClick={() => void status.refetch()}>
          {t("Retry")}
        </Button>
      </Alert>
    );
  return (
    <section
      aria-label={t("Tunnel management")}
      className="min-w-0 space-y-4 border-t border-line pt-4"
    >
      <h3 className="font-medium">{t("Tunnel management")}</h3>
      {status.isError ? (
        <Alert tone="warning" title={t("Could not refresh connectors")}>
          {status.error.message}
        </Alert>
      ) : null}
      {data.connection_error ? (
        <Alert tone="warning" title={t("Cloudflare connection check failed")}>
          {data.connection_error}
        </Alert>
      ) : null}
      {data.replica_error ? (
        <Alert tone="warning" title={t("Local connector check failed")}>
          {data.replica_error}
        </Alert>
      ) : null}
      {data.check_error ? (
        <Alert tone="warning" title={t("Release check failed")}>
          {data.check_error}
        </Alert>
      ) : null}
      <dl className="grid gap-1 text-sm [overflow-wrap:anywhere]">
        <dt className="text-muted">{t("Pinned version")}</dt>
        <dd>{data.pinned_version ?? t("Not pinned yet")}</dd>
        <dt className="text-muted">{t("Latest cloudflared release")}</dt>
        <dd>{data.latest_release?.version ?? t("Unknown")}</dd>
        <dt className="text-muted">{t("Last checked")}</dt>
        <dd>
          {data.checked_at
            ? new Date(data.checked_at).toLocaleString()
            : t("Never")}
        </dd>
      </dl>
      {data.image_overridden ? (
        <Alert tone="warning" title={t("Custom connector image")}>
          {t(
            "DYMINI_PANEL_CLOUDFLARED_IMAGE overrides managed versions. Remove it to enable rolling updates.",
          )}
        </Alert>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button quiet disabled={check.isPending} onClick={() => check.mutate()}>
          {t("Check for updates")}
        </Button>
        <Button
          disabled={
            busy ||
            update.isPending ||
            data.image_overridden ||
            !data.latest_release ||
            (!data.update_available &&
              phase !== "failed" &&
              phase !== "interrupted")
          }
          onClick={() => update.mutate()}
        >
          {busy
            ? t("Updating connectors…")
            : phase === "failed" || phase === "interrupted"
              ? t("Retry update")
              : t("Update connectors")}
        </Button>
      </div>
      <p className="text-sm text-muted">
        {t(
          "A surge connector must register before the old replica is removed. Replicas update one at a time.",
        )}
      </p>
      {data.update ? (
        <div
          role="status"
          aria-live="polite"
          className="space-y-2 rounded-alert border border-line p-3 text-sm [overflow-wrap:anywhere]"
        >
          <Badge value={data.update.status} />
          <p>
            {t(
              "Connector update {version}: {completed}/{total} replicas complete",
              {
                version: data.update.version,
                completed: data.update.completed,
                total: data.update.total,
              },
            )}
          </p>
          {busy ? (
            <p>
              {data.update.replica
                ? t("Waiting for replica {replica} to connect", {
                    replica: data.update.replica,
                  })
                : t("Pulling the target image…")}
            </p>
          ) : null}
          {busy ? (
            <progress
              className="w-full"
              max={data.update.total}
              value={data.update.completed}
              aria-label={t("Connector update progress")}
            />
          ) : null}
          {data.update.message ? (
            <p className="text-danger">{data.update.message}</p>
          ) : null}
        </div>
      ) : null}
      <h4 className="text-sm font-medium">{t("Cloudflare connections")}</h4>
      {data.connections.length === 0 ? (
        <p className="text-sm text-muted">
          {t("No Cloudflare connections reported.")}
        </p>
      ) : (
        <div className="max-w-full overflow-x-auto">
          <table className="w-full min-w-[42rem] text-left text-sm">
            <thead>
              <tr>
                {[
                  t("Connector ID"),
                  t("Version"),
                  t("Origin IP"),
                  t("Colo"),
                  t("Opened at"),
                  t("Status"),
                ].map((heading) => (
                  <th key={heading} className="p-2">
                    {heading}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {data.connections.map((conn, index) => (
                <tr
                  key={`${conn.connector_id}-${conn.connection_id}-${index}`}
                  className="border-t border-line"
                >
                  <td className="max-w-64 break-all p-2 font-mono text-xs">
                    {conn.connector_id}
                  </td>
                  <td className="p-2">{conn.version ?? t("Unknown")}</td>
                  <td className="p-2">{conn.origin_ip ?? "—"}</td>
                  <td className="p-2">{conn.colo ?? "—"}</td>
                  <td className="p-2">
                    {conn.opened_at
                      ? new Date(conn.opened_at).toLocaleString()
                      : "—"}
                  </td>
                  <td className="p-2">
                    <Badge value={conn.healthy ? "healthy" : "reconnecting"} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <h4 className="text-sm font-medium">{t("Local replicas")}</h4>
      {data.replicas.length === 0 ? (
        <p className="text-sm text-muted">
          {t("No local connector containers found.")}
        </p>
      ) : (
        <div className="grid min-w-0 gap-3 md:grid-cols-2">
          {data.replicas.map((replica) => (
            <div
              key={replica.id}
              className="min-w-0 space-y-2 rounded-alert border border-line p-3 text-sm [overflow-wrap:anywhere]"
            >
              <p className="font-medium">{replica.name}</p>
              <Badge value={replica.state} />
              <p>
                {t("Version")}: {replica.version ?? t("Unknown")}
              </p>
              <p>
                {t("Image")}: <code>{replica.image}</code>
              </p>
              <p>
                {t("Digest")}: <code>{replica.digest ?? t("Unknown")}</code>
              </p>
              <p>
                {t("Uptime")}:{" "}
                {replica.uptime_seconds == null
                  ? "—"
                  : t("{seconds} seconds", { seconds: replica.uptime_seconds })}
              </p>
              <p>
                {t("Restarts")}: {replica.restarts}
              </p>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

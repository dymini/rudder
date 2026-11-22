import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";
import { toast } from "sonner";
import { api, download } from "../api";
import {
  errorMessage,
  formatBytes,
  formatDuration,
  formatTime,
} from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";
import { Badge, Button, Card, Dialog, Empty } from "./ui";
import { PanelRestoreHint } from "./backups";

export function PanelBackupStatus({
  health,
}: {
  health: Schemas["PanelBackupHealth"];
}) {
  const t = useT();
  return (
    <div className="space-y-2 text-sm">
      <p>
        {t("Last successful panel backup")}: {formatTime(health.last_success)} ·{" "}
        {t("Age")}:{" "}
        {health.age_seconds == null
          ? "—"
          : formatDuration(health.age_seconds * 1000)}{" "}
        · {t("Failing streak")}: {health.failing_streak}
      </p>
      {health.warning ? (
        <p role="alert" className="break-words text-danger">
          {t(health.warning)}
        </p>
      ) : (
        <Badge value="healthy" />
      )}
    </div>
  );
}

export function PanelBackups() {
  const t = useT();
  const client = useQueryClient();
  const [limit, setLimit] = useState(50);
  const [remove, setRemove] = useState<Schemas["PanelBackupObject"] | null>(
    null,
  );
  const query = useQuery({
    queryKey: ["panel-backups", limit],
    queryFn: () =>
      api<Schemas["PanelBackupInventory"]>(
        `/api/v1/system/panel-backups?limit=${limit}`,
      ),
    refetchInterval: (query) =>
      query.state.data?.objects.some(
        (o) => o.verification?.status === "running",
      )
        ? 3000
        : 60000,
  });
  const destinations = useQuery({
    queryKey: ["s3-destinations"],
    queryFn: () =>
      api<{ id: string; name: string }[]>("/api/v1/s3-destinations"),
  });
  const refresh = () => {
    void client.invalidateQueries({ queryKey: ["panel-backups"] });
    void client.invalidateQueries({ queryKey: ["statistics"] });
  };
  const action = useMutation({
    mutationFn: ({
      url,
      method,
      body,
    }: {
      url: string;
      method: string;
      body?: unknown;
    }) =>
      api(url, {
        method,
        body: body === undefined ? undefined : JSON.stringify(body),
      }),
    onSuccess: () => {
      setRemove(null);
      refresh();
    },
    onError: (e) => toast.error(errorMessage(e, t("Save failed"))),
  });
  const fetchCopy = useMutation({
    mutationFn: (o: Schemas["PanelBackupObject"]) =>
      download(
        `/api/v1/system/panel-backups/${o.run_id}/copies/${o.destination_id}/download`,
      ),
    onError: (e) => toast.error(errorMessage(e, t("Could not load"))),
  });
  return (
    <Card title={t("Panel backups")}>
      <div className="min-w-0 space-y-4" id="panel-backups">
        <p className="text-sm text-muted">
          {t(
            "Managed destination copies are checked remotely. Unknown objects and one-off downloads are not indexed here. Verification never changes the running panel.",
          )}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            quiet
            disabled={query.isFetching}
            onClick={() => {
              void query.refetch();
            }}
          >
            {t("Refresh")}
          </Button>
          <Link
            className="inline-flex min-h-10 items-center text-accent"
            to="/backups"
          >
            {t("Manage backup jobs and running tasks")}
          </Link>
        </div>
        {query.error ? (
          <Empty>{errorMessage(query.error, t("Could not load"))}</Empty>
        ) : null}
        {query.isPending ? <p>{t("Loading…")}</p> : null}
        {query.data ? (
          <>
            <PanelBackupStatus health={query.data.health} />
            <h3 className="font-semibold">{t("Configuration summary")}</h3>
            {query.data.jobs.length === 0 ? (
              <Empty>
                {t(
                  "No panel backup jobs. Create a scheduled full-panel backup below.",
                )}
              </Empty>
            ) : null}
            {query.data.jobs.map((j) => (
              <article
                key={j.id}
                className="min-w-0 space-y-1 rounded-lg border border-line p-3 text-sm"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <h4 className="break-all font-semibold">{j.name}</h4>
                  <Badge value={j.enabled ? "enabled" : "disabled"} />
                  <span>{j.target_kind}</span>
                </div>
                <p className="break-words">
                  {t("Schedule")}: {j.schedule_cron || t("Manual only")} ·{" "}
                  {j.timezone}
                </p>
                <p className="break-all">
                  {t("S3 destinations")}:{" "}
                  {j.destination_ids
                    .map(
                      (id) =>
                        destinations.data?.find((d) => d.id === id)?.name ?? id,
                    )
                    .join(", ")}
                </p>
                <p className="break-all">
                  {t("Retention")}: {JSON.stringify(j.retention)}
                </p>
                <p>
                  {t("Encryption")}:{" "}
                  {j.encrypted ? t("Enabled") : t("Disabled")} ·{" "}
                  {t("Include history")}:{" "}
                  {j.includes_history ? t("Enabled") : t("Disabled")}
                </p>
                <Button
                  quiet
                  disabled={action.isPending}
                  onClick={() =>
                    action.mutate({
                      url: `/api/v1/backup-jobs/${j.id}/run`,
                      method: "POST",
                    })
                  }
                >
                  {t("Run now")}
                </Button>
              </article>
            ))}
            <h3 className="font-semibold">{t("Destination backups")}</h3>
            {query.data.objects.length === 0 ? (
              <Empty>
                {t("No completed panel backup copies are recorded.")}
              </Empty>
            ) : null}
            {query.data.objects.map((o) => (
              <article
                key={`${o.run_id}-${o.destination_id}-${o.key}`}
                className="min-w-0 space-y-2 rounded-lg border border-line p-3 text-sm"
              >
                <div className="flex flex-wrap items-center gap-2">
                  <span>{formatTime(o.created_at)}</span>
                  <span>{formatBytes(o.size_bytes)}</span>
                  <Badge value={o.presence} />
                  <span>Rudder {o.version ?? "—"}</span>
                  <span>
                    {t("Encryption")}:{" "}
                    {o.encrypted ? t("Enabled") : t("Disabled")}
                  </span>
                </div>
                <p className="break-all font-mono text-xs">{o.key}</p>
                <p className="break-all text-xs">
                  {destinations.data?.find((d) => d.id === o.destination_id)
                    ?.name ?? o.destination_id}
                </p>
                <details>
                  <summary className="min-h-10 cursor-pointer py-2">
                    {t("Contents manifest")}
                  </summary>
                  <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all text-xs">
                    {o.manifest
                      ? JSON.stringify(o.manifest, null, 2)
                      : t("Manifest unknown until verification.")}
                  </pre>
                </details>
                {o.verification ? (
                  <div>
                    <p>
                      {t("Verification")}:{" "}
                      <Badge value={o.verification.status} /> ·{" "}
                      {formatTime(
                        o.verification.finished_at ?? o.verification.started_at,
                      )}
                    </p>
                    {o.verification.error ? (
                      <p role="alert" className="break-words text-danger">
                        {o.verification.error}
                      </p>
                    ) : null}
                  </div>
                ) : (
                  <p className="text-muted">{t("Not verified")}</p>
                )}
                <div className="flex flex-wrap gap-2">
                  <Button
                    quiet
                    disabled={fetchCopy.isPending || o.presence !== "present"}
                    onClick={() => fetchCopy.mutate(o)}
                  >
                    {t("Download")}
                  </Button>
                  <Button
                    quiet
                    disabled={
                      action.isPending ||
                      o.presence !== "present" ||
                      query.data.objects.some(
                        (x) => x.verification?.status === "running",
                      )
                    }
                    onClick={() =>
                      action.mutate({
                        url: `/api/v1/system/panel-backups/${o.run_id}/verify`,
                        method: "POST",
                        body: { destination_id: o.destination_id },
                      })
                    }
                  >
                    {t("Verify backup")}
                  </Button>
                  <Button
                    quiet
                    disabled={action.isPending}
                    onClick={() => setRemove(o)}
                  >
                    {t("Delete")}
                  </Button>
                </div>
              </article>
            ))}
            {limit < 200 ? (
              <Button quiet onClick={() => setLimit(Math.min(200, limit + 50))}>
                {t("Load more")}
              </Button>
            ) : null}
          </>
        ) : null}
        <p className="text-xs text-muted">
          {t(
            "Verification checks SHA-256, authenticated decryption, archive contents and pg_restore --list. It does not execute SQL or prove a full disaster recovery. Keep the original passphrase separately.",
          )}
        </p>
        <PanelRestoreHint />
        {remove ? (
          <Dialog
            open
            onClose={() => {
              if (!action.isPending) setRemove(null);
            }}
            title={t("Delete backup copy?")}
          >
            <p className="mb-3 break-all text-sm">{remove.key}</p>
            <p className="mb-3 text-sm text-danger">
              {t(
                "Only this destination copy is deleted. This cannot be undone; keep another verified recovery copy.",
              )}
            </p>
            <Button
              disabled={action.isPending}
              onClick={() =>
                action.mutate({
                  url: `/api/v1/system/panel-backups/${remove.run_id}/copies/${remove.destination_id}`,
                  method: "DELETE",
                })
              }
            >
              {t("Delete")}
            </Button>
          </Dialog>
        ) : null}
      </div>
    </Card>
  );
}

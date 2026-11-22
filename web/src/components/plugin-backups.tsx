import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";
import { toast } from "sonner";
import { api } from "../api";
import { errorMessage, formatBytes, formatDuration, formatTime } from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";
import { Badge, Button, Dialog, Empty, Field, LogPre, TextInput } from "./ui";

type Install = Schemas["PluginInstall"];
type Job = Schemas["BackupJobView"];
type Run = Schemas["BackupRunView"];
const retention = (value: unknown): Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};

export function PluginBackupManagement({ install }: { install: Install }) {
  const t = useT();
  const client = useQueryClient();
  const jobs = useQuery({ queryKey: ["plugin-backups", install.service_id], queryFn: () => api<Job[]>(`/api/v1/backup-jobs?service_id=${install.service_id}`), refetchInterval: 4000 });
  const destinations = useQuery({ queryKey: ["s3-destinations"], queryFn: () => api<{ id: string; name: string }[]>("/api/v1/s3-destinations") });
  const protect = useMutation({
    mutationFn: (enabled: boolean) => api(`/api/v1/plugin-installs/${install.id}/access`, { method: "PUT", body: JSON.stringify({ enabled }) }),
    onSettled: () => { void client.invalidateQueries({ queryKey: ["plugins"] }); },
    onError: (error) => toast.error(errorMessage(error, t("Save failed"))),
  });
  const busy = ["updating", "installing", "restoring"].includes(install.status);
  return <div className="mt-6 min-w-0 space-y-5">
    <section className="space-y-2 border-t border-line pt-4">
      <h3 className="font-semibold">{t("Dashboard protection")}</h3>
      <label className="flex min-h-10 items-center gap-2"><input type="checkbox" checked={install.access_enabled} disabled={protect.isPending || busy} onChange={(e) => protect.mutate(e.target.checked)} />{t("Protect the dashboard with Rudder Access")}</label>
      <p className="text-xs text-muted">{t("All other paths require Access. These API prefixes bypass Access but still require Dockside's bearer token:")}</p>
      <p className="break-all font-mono text-xs">{install.access_bypass_paths.join(", ")}</p>
      <p className="text-xs text-muted">{t("Configure Access sign-in before opening the dashboard. Existing Access rules are preserved.")}</p>
      {!install.access_enabled ? <p role="alert" className="text-sm text-danger">{t("Access protection is off. Anyone can open the dashboard login; API bearer authentication is unchanged.")}</p> : null}
    </section>
    <section className="space-y-3 border-t border-line pt-4">
      <div className="flex flex-wrap items-center gap-2"><h3 className="font-semibold">{t("Attached backup jobs")}</h3><Badge value={install.status} /></div>
      {install.last_error ? <p role="alert" className="whitespace-pre-wrap break-words text-sm text-danger">{install.last_error}</p> : null}
      {install.safety_run_id ? <p className="break-all text-xs">{t("Last safety backup")}: {install.safety_run_id}</p> : null}
      {jobs.error ? <Empty>{errorMessage(jobs.error, t("Could not load"))}</Empty> : null}
      {jobs.data?.length === 0 ? <Empty>{t("No backup jobs attached. Set one up above.")}</Empty> : null}
      {jobs.data?.map((job) => <PluginJob key={job.id} job={job} install={install} busy={busy} destinations={destinations.data ?? []} />)}
    </section>
  </div>;
}

function PluginJob({ job, install, busy, destinations }: { job: Job; install: Install; busy: boolean; destinations: { id: string; name: string }[] }) {
  const t = useT();
  const client = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [schedule, setSchedule] = useState(job.schedule_cron ?? "");
  const [keep, setKeep] = useState(Number(retention(job.retention).keep_last ?? 7));
  const [selected, setSelected] = useState<Run | null>(null);
  const [limit, setLimit] = useState(20);
  const runs = useQuery({ queryKey: ["plugin-backup-runs", job.id, limit], queryFn: () => api<Run[]>(`/api/v1/backup-jobs/${job.id}/runs?limit=${limit}`), refetchInterval: 4000 });
  const refresh = () => { void client.invalidateQueries({ queryKey: ["plugin-backups"] }); void client.invalidateQueries({ queryKey: ["plugin-backup-runs", job.id] }); void client.invalidateQueries({ queryKey: ["plugins"] }); };
  const run = useMutation({ mutationFn: () => api(`/api/v1/backup-jobs/${job.id}/run`, { method: "POST" }), onSuccess: refresh, onError: (error) => toast.error(errorMessage(error, t("Backup failed"))) });
  const save = useMutation({
    mutationFn: () => api(`/api/v1/backup-jobs/${job.id}`, { method: "PATCH", body: JSON.stringify({ schedule_cron: schedule, retention: { ...retention(job.retention), keep_last: keep } }) }),
    onSuccess: () => { setEditing(false); refresh(); }, onError: (error) => toast.error(errorMessage(error, t("Save failed"))),
  });
  const restore = useMutation({
    mutationFn: (run: Run) => api(`/api/v1/plugin-installs/${install.id}/restore`, { method: "POST", body: JSON.stringify({ run_id: run.id }) }),
    onSuccess: () => { setSelected(null); refresh(); toast.success(t("Plugin restore started")); }, onError: (error) => toast.error(errorMessage(error, t("Restore failed"))),
  });
  const active = runs.data?.some((run) => ["queued", "running"].includes(run.status)) ?? false;
  return <article className="min-w-0 rounded-lg border border-line p-3 text-sm">
    <div className="flex flex-wrap items-center gap-2"><h4 className="min-w-0 break-all font-semibold">{job.name}</h4><Badge value={job.enabled ? "enabled" : "disabled"} /></div>
    <p className="break-words text-xs text-muted">{t("Schedule")}: {job.schedule_cron || t("Manual only")} · {job.timezone} · {t("Keep last")}: {String(retention(job.retention).keep_last ?? "—")}</p>
    <p className="break-all text-xs text-muted">{t("S3 destinations")}: {job.destination_ids.map((id) => destinations.find((d) => d.id === id)?.name ?? id).join(", ")}</p>
    <p className="break-all text-xs text-muted">{t("Retention")}: {JSON.stringify(job.retention)}</p>
    <div className="my-2 flex flex-wrap gap-2"><Button quiet disabled={busy || active || run.isPending} onClick={() => run.mutate()}>{t("Run now")}</Button><Button quiet disabled={busy || active} onClick={() => { setSchedule(job.schedule_cron ?? ""); setKeep(Number(retention(job.retention).keep_last ?? 7)); setEditing(!editing); }}>{t("Edit schedule and retention")}</Button><Link className="inline-flex min-h-10 items-center text-accent" to={`/services/${install.service_id}?tab=backups`}>{t("All backup settings")}</Link></div>
    {editing ? <form className="my-3 grid gap-3 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}><Field label={t("Schedule")} hint={t("Leave blank for manual backups.")}><TextInput value={schedule} onChange={(e) => setSchedule(e.target.value)} /></Field><Field label={t("Keep last")}><TextInput type="number" required min={1} value={keep} onChange={(e) => setKeep(Number(e.target.value))} /></Field><Button type="submit" disabled={save.isPending || busy || active}>{t("Save")}</Button></form> : null}
    <h5 className="mt-3 font-medium">{t("Run history")}</h5>
    {runs.error ? <p role="alert" className="break-words text-danger">{errorMessage(runs.error, t("Could not load"))}</p> : null}
    {runs.data?.length === 0 ? <p className="text-muted">{t("No runs yet.")}</p> : null}
    <div className="divide-y divide-line">{runs.data?.map((item) => <div key={item.id} className="min-w-0 space-y-2 py-3">
      <div className="flex flex-wrap items-center gap-2"><span>{formatTime(item.started_at)}</span><Badge value={item.status} /><span>{formatBytes(item.size_bytes)} · {formatDuration(item.duration_ms)}</span><span className="text-xs">{item.kind} · {item.trigger}</span></div>
      <p className="break-all font-mono text-xs">{item.id}</p>
      {item.error ? <p role="alert" className="whitespace-pre-wrap break-words text-danger">{item.error}</p> : null}
      {item.kind === "backup" && ["succeeded", "partial"].includes(item.status) && job.target_kind === "volume" ? <Button quiet disabled={busy || active || restore.isPending} onClick={() => setSelected(item)}>{t("Restore into this install")}</Button> : null}
      <details><summary className="min-h-10 cursor-pointer py-2">{t("Logs")}</summary><LogPre empty={t("No log output yet.")} className="mt-2">{item.logs}</LogPre></details>
    </div>)}</div>
    {runs.data?.length === limit && limit < 500 ? <Button quiet onClick={() => setLimit(Math.min(500, limit + 50))}>{t("Load more")}</Button> : null}
    {selected ? <Dialog open onClose={() => { if (!restore.isPending) setSelected(null); }} title={t("Restore into this install")}><p className="mb-3 text-sm">{t("Rudder stops the service, saves a separate unfiltered safety backup, replaces the data volume, starts the service and checks health. A failed restore rolls back to the safety copy.")}</p><p className="mb-3 break-all font-mono text-xs">{selected.id}</p><Button disabled={restore.isPending || busy || active} onClick={() => restore.mutate(selected)}>{t("Restore")}</Button></Dialog> : null}
  </article>;
}

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { PluginBackupManagement } from "../components/plugin-backups";
import { Link } from "react-router-dom";
import { toast } from "sonner";
import { api } from "../api";
import { Badge, Button, Card, Dialog, Empty, Field, NameInput, Select, TextArea, TextInput } from "../components/ui";
import { errorMessage } from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";

type Plugin = Schemas["PluginView"];
type Install = Schemas["PluginInstall"];
type Named = { id: string; name: string };
type Backup = { destination_ids: string[]; schedule: string; keep_last: number };

export function PluginsPage() {
  const t = useT();
  const client = useQueryClient();
  const plugins = useQuery({ queryKey: ["plugins"], queryFn: () => api<Plugin[]>("/api/v1/plugins"), refetchInterval: 4000 });
  const [chosen, setChosen] = useState<string | null>(null);
  const [target, setTarget] = useState<{ plugin: Plugin; install: Install } | null>(null);
  const [backup, setBackup] = useState<Install | null>(null);
  const check = useMutation({
    mutationFn: (slug: string) => api<Schemas["PluginCheck"]>(`/api/v1/plugins/${slug}/check`, { method: "POST" }),
    onSuccess: (result) => {
      void client.invalidateQueries({ queryKey: ["plugins"] });
      if (result.error) toast.error(result.error); else toast.success(t("Plugin releases checked"));
    },
    onError: (error) => toast.error(errorMessage(error, t("Could not load"))),
  });
  const plugin = plugins.data?.find((p) => p.manifest.slug === chosen);
  const backupInstall = plugins.data?.flatMap((p) => p.installs).find((i) => i.id === backup?.id) ?? backup;
  return <>
    <h1 className="text-2xl font-semibold">{t("Plugins")}</h1>
    <p className="text-sm text-muted">{t("Install managed stacks from verified GitHub release descriptors. Back up data before updating.")}</p>
    {plugins.error ? <Empty>{errorMessage(plugins.error, t("Could not load"))}</Empty> : null}
    {plugins.data?.length === 0 ? <Empty>{t("No plugins available.")}</Empty> : null}
    {plugins.data?.map((p) => <Card key={p.manifest.slug} title={p.manifest.name}>
      <p className="text-sm text-muted">{p.manifest.description}</p>
      <a className="text-sm text-accent" href={`https://github.com/${p.repo}`} target="_blank" rel="noreferrer">{p.repo}</a>
      <p className="my-2 text-sm">{t("Latest version")}: {p.last_check?.latest_version ?? "—"}</p>
      {p.last_check ? <p className="text-xs text-muted">{t("Last checked")}: {new Date(p.last_check.checked_at).toLocaleString()}</p> : null}
      {p.last_check?.error ? <p role="alert" className="text-sm text-danger">{p.last_check.error}</p> : null}
      <div className="my-3 flex gap-2">
        <Button quiet disabled={check.isPending} onClick={() => check.mutate(p.manifest.slug)}>{t("Check for updates")}</Button>
        <Button disabled={!p.last_check?.releases.length} onClick={() => setChosen(p.manifest.slug)}>{t("Install")}</Button>
      </div>
      <div className="divide-y divide-line">
        {p.installs.map((i) => <div key={i.id} className="flex flex-wrap items-center gap-3 py-3 text-sm">
          <Link className="text-accent" to={`/services/${i.service_id}`}>{i.service_name}</Link>
          <span>{i.environment_name} · {i.server_name} · {i.installed_version}</span><Badge value={i.status} />
          <Link className="text-accent" to={`/services/${i.service_id}?tab=environment`}>{t("Environment")}</Link>
          {i.backup_job_id ? <Link className="text-accent" to={`/services/${i.service_id}?tab=backups`}>{t("Backup job")} · {i.backup_job_id.slice(0, 8)}</Link> : null}
          <Button quiet onClick={() => setBackup(i)}>{t("Plugin settings")}</Button>
          {i.update_available ? <Button disabled={["updating", "installing", "restoring"].includes(i.status)} onClick={() => setTarget({ plugin: p, install: i })}>{t("Update to {version}", { version: i.latest_version ?? "" })}</Button> : null}
          {i.last_error ? <p role="alert" className="w-full whitespace-pre-wrap text-danger">{i.last_error}</p> : null}
        </div>)}
      </div>
    </Card>)}
    {plugin ? <InstallDialog plugin={plugin} onClose={() => setChosen(null)} /> : null}
    {target ? <UpdateDialog install={target.install} onClose={() => setTarget(null)} /> : null}
    {backupInstall ? <BackupDialog install={backupInstall} onClose={() => setBackup(null)} /> : null}
  </>;
}

function BackupFields({ value, onChange }: { value: Backup; onChange: (value: Backup) => void }) {
  const t = useT();
  const destinations = useQuery({ queryKey: ["s3-destinations"], queryFn: () => api<Named[]>("/api/v1/s3-destinations") });
  return <>
    <fieldset className="space-y-2"><legend className="text-sm font-medium">{t("S3 destinations")}</legend>
      {destinations.error ? <p role="alert">{errorMessage(destinations.error, t("Could not load"))}</p> : null}
      {destinations.data?.length === 0 ? <Link className="text-sm text-accent" to="/s3-destinations">{t("Create an S3 destination first.")}</Link> : null}
      {destinations.data?.map((d) => <label key={d.id} className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={value.destination_ids.includes(d.id)} onChange={(e) => onChange({ ...value, destination_ids: e.target.checked ? [...value.destination_ids, d.id] : value.destination_ids.filter((id) => id !== d.id) })} />{d.name}
      </label>)}
    </fieldset>
    <Field label={t("Schedule")}><TextInput required value={value.schedule} onChange={(e) => onChange({ ...value, schedule: e.target.value })} /></Field>
    <Field label={t("Keep last")}><TextInput type="number" min={1} required value={value.keep_last} onChange={(e) => onChange({ ...value, keep_last: Number(e.target.value) })} /></Field>
    <p className="text-xs text-muted">{t("Backups stop the service for SQLite consistency. Updates wait for a successful backup.")}</p>
  </>;
}

function InstallDialog({ plugin, onClose }: { plugin: Plugin; onClose: () => void }) {
  const t = useT();
  const client = useQueryClient();
  const projects = useQuery({ queryKey: ["projects"], queryFn: () => api<Named[]>("/api/v1/projects") });
  const servers = useQuery({ queryKey: ["servers"], queryFn: () => api<Named[]>("/api/v1/servers") });
  const [projectId, setProjectId] = useState("");
  const project = projectId || projects.data?.[0]?.id || "";
  const environments = useQuery({ queryKey: ["environments", project], queryFn: () => api<Named[]>(`/api/v1/projects/${project}/environments`), enabled: Boolean(project) });
  const [environmentId, setEnvironmentId] = useState("");
  const environment = environmentId || environments.data?.[0]?.id || "";
  const [serverId, setServerId] = useState("");
  const server = serverId || servers.data?.[0]?.id || "";
  const [name, setName] = useState(plugin.manifest.slug);
  const [version, setVersion] = useState(plugin.last_check?.latest_version ?? "");
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(plugin.manifest.variables.map((v) => [v.name, v.default ?? ""])));
  const [withBackup, setWithBackup] = useState(false);
  const [backup, setBackup] = useState<Backup>({ destination_ids: [], schedule: "@daily", keep_last: 7 });
  const [result, setResult] = useState<Schemas["PluginInstalled"] | null>(null);
  const deploy = useMutation({
    mutationFn: () => api<Schemas["PluginInstalled"]>(`/api/v1/environments/${environment}/plugins/${plugin.manifest.slug}/install`, {
      method: "POST", body: JSON.stringify({ name, server_id: server, version, values: Object.fromEntries(Object.entries(values).filter(([, v]) => v !== "")), backup: withBackup ? backup : undefined }),
    }),
    onSuccess: (result) => { setResult(result); void client.invalidateQueries({ queryKey: ["plugins"] }); },
    onError: (error) => toast.error(errorMessage(error, t("Deploy failed"))),
  });
  return <Dialog open onClose={onClose} title={t("Install {name}", { name: plugin.manifest.name })} variant="drawer" wide>
    {result ? <div className="space-y-4">
      <p>{t("Generated secrets — copy them now, they are not shown again:")}</p>
      <pre className="whitespace-pre-wrap break-all rounded bg-paper p-3 text-xs">{result.generated.map((v) => `${v.key}=${v.value}`).join("\n")}</pre>
      <Link className="text-accent" to={`/services/${result.service.id}`}>{t("Open service")}</Link>
      <Button quiet onClick={onClose}>{t("Close")}</Button>
    </div> : <form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); deploy.mutate(); }}>
      <Field label={t("Project")}><Select value={project} onChange={(e) => { setProjectId(e.target.value); setEnvironmentId(""); }}>{projects.data?.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}</Select></Field>
      <Field label={t("Environment")}><Select value={environment} onChange={(e) => setEnvironmentId(e.target.value)}>{environments.data?.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}</Select></Field>
      <Field label={t("Server")}><Select value={server} onChange={(e) => setServerId(e.target.value)}>{servers.data?.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}</Select></Field>
      <Field label={t("Service name")}><NameInput required value={name} onChange={(e) => setName(e.target.value)} /></Field>
      <Field label={t("Version")}><Select value={version} onChange={(e) => setVersion(e.target.value)}>{plugin.last_check?.releases.map((r) => <option key={r.version} value={r.version}>{r.version}</option>)}</Select></Field>
      {plugin.manifest.variables.map((v) => <Field key={v.name} label={`${v.label}${v.required ? " *" : ""}`}>
        {v.kind === "text" ? <TextArea aria-label={v.label} value={values[v.name]} placeholder={v.description} onChange={(e) => setValues({ ...values, [v.name]: e.target.value })} />
          : v.kind === "boolean" ? <Select value={values[v.name]} onChange={(e) => setValues({ ...values, [v.name]: e.target.value })}><option value="true">{t("Yes")}</option><option value="false">{t("No")}</option></Select>
          : <TextInput required={v.required} autoComplete="off" type={v.kind === "secret" ? "password" : v.kind === "number" ? "number" : "text"} placeholder={v.description} value={values[v.name]} onChange={(e) => setValues({ ...values, [v.name]: e.target.value })} />}
      </Field>)}
      <p className="text-xs text-muted">{plugin.manifest.notes}</p>
      <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={withBackup} onChange={(e) => setWithBackup(e.target.checked)} />{t("Set up backup")}</label>
      {withBackup ? <BackupFields value={backup} onChange={setBackup} /> : null}
      <Button type="submit" disabled={!environment || !server || !version || deploy.isPending || (withBackup && backup.destination_ids.length === 0)}>{t("Install")}</Button>
    </form>}
  </Dialog>;
}

function UpdateDialog({ install, onClose }: { install: Install; onClose: () => void }) {
  const t = useT();
  const client = useQueryClient();
  const pending = install.pending_releases;
  const update = useMutation({
    mutationFn: () => api(`/api/v1/plugin-installs/${install.id}/update`, { method: "POST", body: "{}" }),
    onSuccess: () => { void client.invalidateQueries({ queryKey: ["plugins"] }); onClose(); },
    onError: (error) => toast.error(errorMessage(error, t("Update failed"))),
  });
  return <Dialog open onClose={onClose} title={t("Update to {version}", { version: install.latest_version ?? "" })} variant="drawer">
    <p className="mb-3 text-sm">{t(install.backup_job_id ? "The backup must succeed before the image changes." : "No backup job is attached. Set up a backup before updating important data.")}</p>
    {pending.map((r) => <section key={r.version} className="mb-4"><h3 className="font-semibold">{r.version}</h3><pre className="whitespace-pre-wrap text-sm">{r.notes}</pre></section>)}
    <Button disabled={update.isPending} onClick={() => update.mutate()}>{t("Update")}</Button>
  </Dialog>;
}

function BackupDialog({ install, onClose }: { install: Install; onClose: () => void }) {
  const t = useT();
  const client = useQueryClient();
  const [backup, setBackup] = useState<Backup>({ destination_ids: [], schedule: "@daily", keep_last: 7 });
  const save = useMutation({
    mutationFn: () => api(`/api/v1/plugin-installs/${install.id}/backup`, { method: "POST", body: JSON.stringify(backup) }),
    onSuccess: () => { void client.invalidateQueries({ queryKey: ["plugins"] }); onClose(); },
    onError: (error) => toast.error(errorMessage(error, t("Save failed"))),
  });
  return <Dialog open onClose={onClose} title={t("Plugin settings")} variant="drawer"><h3 className="mb-3 font-semibold">{t("Set up backup")}</h3><form className="grid gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}><BackupFields value={backup} onChange={setBackup} /><Button type="submit" disabled={save.isPending || backup.destination_ids.length === 0 || ["updating", "installing", "restoring"].includes(install.status)}>{t("Save")}</Button></form><PluginBackupManagement install={install} /></Dialog>;
}

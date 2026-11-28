import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { toast } from "sonner";
import { api } from "../api";
import { cachePercent } from "../build-hosts";
import {
  Badge,
  Button,
  Card,
  CodeBlock,
  Dialog,
  Empty,
  Field,
  NameInput,
  TextInput,
} from "../components/ui";
import { errorMessage, formatBytes, formatTime } from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";

type Builder = Schemas["BuilderView"];
type Inventory = Schemas["BuilderInventory"];

function BuilderDetails({
  builder,
  onClose,
}: {
  builder: Builder;
  onClose: () => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const [enabled, setEnabled] = useState(builder.enabled);
  const [paused, setPaused] = useState(builder.paused);
  const [draining, setDraining] = useState(builder.draining);
  const [maxJobs, setMaxJobs] = useState(String(builder.max_jobs));
  const [limit, setLimit] = useState(
    String(builder.cache_limit_bytes / 1024 ** 3),
  );
  const cache = useQuery({
    queryKey: ["builder-cache", builder.id],
    queryFn: () =>
      api<Schemas["BuilderCacheEntry"][]>(
        `/api/v1/builders/${builder.id}/cache`,
      ),
    refetchInterval: 5000,
  });
  const jobs = useQuery({
    queryKey: ["builder-jobs", builder.id],
    queryFn: () =>
      api<Schemas["BuilderJob"][]>(`/api/v1/builders/${builder.id}/jobs`),
    refetchInterval: 5000,
  });
  const save = useMutation({
    mutationFn: () =>
      api(`/api/v1/builders/${builder.id}`, {
        method: "PATCH",
        body: JSON.stringify({
          enabled,
          paused,
          draining,
          max_jobs: Number(maxJobs),
          cache_limit_bytes: Math.round(Number(limit) * 1024 ** 3),
        }),
      }),
    onSuccess: async () => {
      toast.success(t("Build host settings saved"));
      await client.invalidateQueries({ queryKey: ["builders"] });
    },
    onError: (error) =>
      toast.error(errorMessage(error, t("Could not save build host settings"))),
  });
  const prune = useMutation({
    mutationFn: ({ filter, lru }: { filter?: string; lru?: boolean }) =>
      api<Schemas["BuilderCachePruned"]>(
        `/api/v1/builders/${builder.id}/cache${lru ? "/prune" : filter ? `?${filter}` : ""}`,
        { method: lru ? "POST" : "DELETE" },
      ),
    onSuccess: async (result) => {
      toast.success(
        t("Reclaimed {size}", { size: formatBytes(result.reclaimed_bytes) }),
      );
      await Promise.all([
        client.invalidateQueries({ queryKey: ["builders"] }),
        client.invalidateQueries({ queryKey: ["builder-cache", builder.id] }),
      ]);
    },
    onError: (error) =>
      toast.error(errorMessage(error, t("Could not prune builder cache"))),
  });
  return (
    <Dialog
      open
      onClose={onClose}
      title={t("Manage build host: {name}", { name: builder.name })}
      variant="drawer"
      wide
    >
      <div className="space-y-6">
        <form
          className="grid gap-4 sm:grid-cols-2"
          onSubmit={(event: FormEvent) => {
            event.preventDefault();
            save.mutate();
          }}
        >
          <div className="flex flex-wrap gap-4 sm:col-span-2">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => setEnabled(e.target.checked)}
              />
              {t("Builder enabled")}
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={paused}
                onChange={(e) => setPaused(e.target.checked)}
              />
              {t("Paused")}
            </label>
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={draining}
                onChange={(e) => setDraining(e.target.checked)}
              />
              {t("Draining")}
            </label>
          </div>
          <Field
            label={t("Maximum build jobs")}
            hint={t(
              "Run 1–32 builds at once. Each build needs Docker resources.",
            )}
          >
            <TextInput
              type="number"
              min={1}
              max={32}
              required
              value={maxJobs}
              onChange={(e) => setMaxJobs(e.target.value)}
            />
          </Field>
          <Field
            label={t("Cache limit (GiB)")}
            hint={t(
              "Zero removes inactive caches on the next prune. Active caches are protected.",
            )}
          >
            <TextInput
              type="number"
              min={0}
              step="any"
              required
              value={limit}
              onChange={(e) => setLimit(e.target.value)}
            />
          </Field>
          <p className="text-sm text-muted sm:col-span-2">
            {t(
              "Pausing, draining or disabling stops new claims; running jobs finish.",
            )}
          </p>
          <p className="text-sm text-muted sm:col-span-2">
            {t(
              "The agent also needs builder mode. Re-run its installer with --builder --no-edge, then check dymini-panel-agent status.",
            )}
          </p>
          <div>
            <Button type="submit" disabled={save.isPending}>
              {t("Save settings")}
            </Button>
          </div>
        </form>
        <section className="space-y-3">
          <h2 className="font-semibold">{t("Build jobs")}</h2>
          {jobs.isLoading ? <p>{t("Loading…")}</p> : null}
          {jobs.isError ? (
            <p role="alert" className="text-danger">
              {errorMessage(jobs.error, t("Could not load build jobs"))}
            </p>
          ) : null}
          {jobs.data?.length === 0 ? (
            <p className="text-sm text-muted">{t("No build jobs yet.")}</p>
          ) : null}
          <ul className="divide-y divide-line">
            {jobs.data?.map((job) => (
              <li
                key={job.id}
                className="flex flex-wrap items-center justify-between gap-2 py-2 text-sm"
              >
                <div>
                  <Link
                    className="font-medium underline"
                    to={`/services/${job.service_id}?tab=deployments`}
                  >
                    {job.service_name}
                  </Link>
                  <p className="text-xs text-muted">
                    {formatTime(job.created_at)}
                  </p>
                </div>
                <Badge value={job.status} />
              </li>
            ))}
          </ul>
        </section>
        <section className="space-y-3">
          <h2 className="font-semibold">{t("Build cache")}</h2>
          <p className="text-sm text-muted">
            {t(
              "Last reported cache entries remain visible offline. Connect the host to clear or prune; active-service caches are protected.",
            )}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              quiet
              disabled={!builder.online || prune.isPending}
              onClick={() => {
                if (
                  window.confirm(
                    t(
                      "Prune this build host's cache to its saved limit? Active caches are kept.",
                    ),
                  )
                )
                  prune.mutate({ lru: true });
              }}
            >
              {t("Prune to limit")}
            </Button>
            <Button
              variant="danger"
              disabled={!builder.online || prune.isPending}
              onClick={() => {
                if (
                  window.confirm(
                    t(
                      "Clear all inactive caches on this build host? Future builds may be slower.",
                    ),
                  )
                )
                  prune.mutate({});
              }}
            >
              {t("Clear all caches")}
            </Button>
          </div>
          {cache.isLoading ? <p>{t("Loading…")}</p> : null}
          {cache.isError ? (
            <p role="alert" className="text-danger">
              {errorMessage(cache.error, t("Could not load builder cache"))}
            </p>
          ) : null}
          {cache.data?.length === 0 ? (
            <p className="text-sm text-muted">{t("No cached builds yet.")}</p>
          ) : null}
          {cache.data?.length ? (
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-line">
                    <th className="py-2 pr-3">{t("Service")}</th>
                    <th className="pr-3">{t("Cache key")}</th>
                    <th className="pr-3">{t("Size")}</th>
                    <th className="pr-3">{t("Last used")}</th>
                    <th>{t("Actions")}</th>
                  </tr>
                </thead>
                <tbody>
                  {cache.data.map((entry) => (
                    <tr
                      key={`${entry.service_id}/${entry.cache_key}`}
                      className="border-b border-line"
                    >
                      <td className="py-3 pr-3">{entry.service_name}</td>
                      <td className="max-w-44 break-all pr-3 font-mono text-xs">
                        {entry.cache_key}
                      </td>
                      <td className="whitespace-nowrap pr-3">
                        {formatBytes(entry.size_bytes)}
                      </td>
                      <td className="pr-3">{formatTime(entry.last_used_at)}</td>
                      <td>
                        <Button
                          quiet
                          disabled={!builder.online || prune.isPending}
                          onClick={() => {
                            if (
                              window.confirm(
                                t(
                                  "Clear this cache entry? Future builds may be slower.",
                                ),
                              )
                            )
                              prune.mutate({
                                filter: new URLSearchParams({
                                  service_id: entry.service_id,
                                  key: entry.cache_key,
                                }).toString(),
                              });
                          }}
                        >
                          {t("Clear")}
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}
        </section>
      </div>
    </Dialog>
  );
}

export function BuildersPage() {
  const t = useT();
  const client = useQueryClient();
  const inventory = useQuery({
    queryKey: ["builders"],
    queryFn: () => api<Inventory>("/api/v1/builders"),
    refetchInterval: 3000,
  });
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [created, setCreated] = useState<Schemas["CreatedBuilder"] | null>(
    null,
  );
  const [selected, setSelected] = useState<string | null>(null);
  const create = useMutation({
    mutationFn: () =>
      api<Schemas["CreatedBuilder"]>("/api/v1/builders", {
        method: "POST",
        body: JSON.stringify({ name }),
      }),
    onSuccess: async (result) => {
      setCreated(result);
      setName("");
      await client.invalidateQueries({ queryKey: ["builders"] });
    },
    onError: (error) =>
      toast.error(errorMessage(error, t("Could not add build host"))),
  });
  const builders = inventory.data?.builders ?? [];
  const chosen = builders.find((builder) => builder.id === selected);
  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">{t("Builders")}</h1>
        <Button
          icon="plus"
          onClick={() => {
            setCreated(null);
            setAdding(true);
          }}
        >
          {t("Add build host")}
        </Button>
      </div>
      <p className="text-sm text-muted">
        {t(
          "Trusted remote machines build images and return them for health-checked deployment. Existing remote servers are listed too.",
        )}{" "}
        <a
          className="underline"
          href="https://github.com/dymini/panel/blob/main/docs/build-hosts.md"
          target="_blank"
          rel="noreferrer"
        >
          {t("Build host guide")}
        </a>
      </p>
      {inventory.isLoading ? <p>{t("Loading…")}</p> : null}
      {inventory.isError ? (
        <p role="alert" className="text-danger">
          {errorMessage(inventory.error, t("Could not load build hosts"))}
        </p>
      ) : null}
      <Card title={t("Build hosts")} flush>
        {!inventory.isLoading && !inventory.isError && builders.length === 0 ? (
          <Empty icon="server" title={t("No build hosts yet")}>
            {t(
              "Add a machine or enable builder mode on an existing remote server.",
            )}
          </Empty>
        ) : null}
        <ul className="divide-y divide-line">
          {builders.map((builder) => (
            <li
              key={builder.id}
              className="grid gap-4 p-5 md:grid-cols-[1fr_minmax(12rem,18rem)_auto] md:items-center"
            >
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 className="font-semibold">{builder.name}</h2>
                  <Badge value={builder.online ? "online" : "offline"} />
                  {!builder.enabled ? (
                    <Badge value={t("Builder disabled")} tone="outline" />
                  ) : null}
                  {builder.paused ? (
                    <Badge value={t("Paused")} tone="outline" />
                  ) : null}
                  {builder.draining ? (
                    <Badge value={t("Draining")} tone="outline" />
                  ) : null}
                </div>
                <p className="mt-1 text-xs text-muted">
                  {t("Last seen: {time}", {
                    time: builder.last_seen
                      ? formatTime(builder.last_seen)
                      : t("Never"),
                  })}{" "}
                  ·{" "}
                  {t("Agent: {version}", {
                    version: builder.version ?? t("Unknown"),
                  })}{" "}
                  · {builder.arch ?? t("Unknown")}
                </p>
                <p className="mt-1 text-sm text-muted">
                  {builder.buildx
                    ? t("Buildx available")
                    : t("Buildx unavailable")}{" "}
                  ·{" "}
                  {builder.nixpacks
                    ? t("Nixpacks available")
                    : t("Nixpacks unavailable")}
                </p>
                <p className="mt-1 text-sm">
                  {t("{running} running · {queued} queued", {
                    running: builder.active_jobs,
                    queued: builder.queued_jobs,
                  })}
                </p>
              </div>
              <div className="space-y-2">
                <p className="text-xs text-muted">
                  {t("Cache: {used} / {limit}", {
                    used: formatBytes(builder.cache_bytes),
                    limit: formatBytes(builder.cache_limit_bytes),
                  })}
                </p>
                <progress
                  className="h-2 w-full"
                  max={100}
                  value={cachePercent(
                    builder.cache_bytes,
                    builder.cache_limit_bytes,
                  )}
                  aria-label={t("Build cache usage")}
                />
              </div>
              <Button quiet onClick={() => setSelected(builder.id)}>
                {t("Manage build host")}
              </Button>
            </li>
          ))}
        </ul>
      </Card>
      <Card
        title={t("Waiting builds")}
        description={t(
          "Showing up to 200 waiting builds. Auto builds choose a host only when claimed.",
        )}
      >
        {inventory.data?.waiting_jobs.length === 0 ? (
          <p className="text-sm text-muted">
            {t("No builds waiting for placement.")}
          </p>
        ) : null}
        <ul className="divide-y divide-line">
          {inventory.data?.waiting_jobs.map((job) => (
            <li
              key={job.id}
              className="flex flex-wrap justify-between gap-2 py-2 text-sm"
            >
              <Link
                className="underline"
                to={`/services/${job.service_id}?tab=deployments`}
              >
                {job.service_name}
              </Link>
              <span className="text-muted">
                {t("Queued {time}", { time: formatTime(job.created_at) })}
              </span>
              <Badge value={job.status} />
            </li>
          ))}
        </ul>
      </Card>
      <Dialog
        open={adding}
        onClose={() => setAdding(false)}
        title={t("Add build host")}
      >
        {created ? (
          <div className="space-y-4">
            <p className="text-sm">
              {t(
                "Run this command as root on {name}. It starts no edge containers.",
                { name: created.server.name },
              )}
            </p>
            <CodeBlock copy>{created.join_command}</CodeBlock>
            <p className="text-sm text-muted">
              {t("The token works once and expires {time}.", {
                time: formatTime(created.expires_at),
              })}
            </p>
            <p className="text-sm text-muted">
              {t(
                "Append --with-nixpacks if this host should build Nixpacks apps.",
              )}
            </p>
            <p className="text-sm">
              {t(
                "Run dymini-panel-agent status on the machine to check. This list updates automatically.",
              )}
            </p>
            <Button onClick={() => setAdding(false)}>{t("Done")}</Button>
          </div>
        ) : (
          <form
            className="grid gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              create.mutate();
            }}
          >
            <Field label={t("Name")}>
              <NameInput
                required
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            <p className="text-sm text-muted">
              {t(
                "The builder receives source and build secrets. Only enroll machines you trust.",
              )}
            </p>
            <Button type="submit" disabled={create.isPending}>
              {t("Add build host")}
            </Button>
          </form>
        )}
      </Dialog>
      {chosen ? (
        <BuilderDetails
          key={chosen.id}
          builder={chosen}
          onClose={() => setSelected(null)}
        />
      ) : null}
    </>
  );
}

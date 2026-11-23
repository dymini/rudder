import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";
import { toast } from "sonner";
import { api } from "../api";
import { DomainRouting } from "../components/domain-routing";
import { Badge, Button, Dialog, Empty, Field, Select } from "../components/ui";
import { errorMessage } from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";

type Domain = Schemas["GlobalDomainView"];

export function DomainsPage() {
  const t = useT();
  const client = useQueryClient();
  const [editing, setEditing] = useState<Domain | null>(null);
  const [zoneFilter, setZoneFilter] = useState("");
  const domains = useQuery({
    queryKey: ["global-domains"],
    queryFn: () => api<Domain[]>("/api/v1/domains"),
  });
  const remove = useMutation({
    mutationFn: (id: string) =>
      api(`/api/v1/domains/${id}`, { method: "DELETE" }),
    onSuccess: async () => {
      toast.success(t("Domain removed"));
      await client.invalidateQueries({ queryKey: ["global-domains"] });
      await client.invalidateQueries({ queryKey: ["domains"] });
    },
    onError: (error) =>
      toast.error(errorMessage(error, t("Could not remove the domain"))),
  });
  const zoneChoices = [...new Map((domains.data ?? []).filter((d) => d.zone_id).map((d) => [d.zone_id!, d.zone_name!])).entries()].sort((a, b) => a[1].localeCompare(b[1]));
  const filtered = domains.data?.filter((domain) => !zoneFilter || (zoneFilter === "unmatched" ? !domain.zone_id : domain.zone_id === zoneFilter));
  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-semibold">{t("Domains")}</h1>
        <Button
          quiet
          disabled={domains.isFetching}
          onClick={() => void domains.refetch()}
        >
          {t("Re-check DNS")}
        </Button>
      </div>
      <p className="text-sm text-muted">
        {t("Every service, panel, and Access hostname across your servers.")}
      </p>
      <div className="max-w-sm"><Field label={t("Filter by zone")}><Select value={zoneFilter} onChange={(event) => setZoneFilter(event.target.value)}>
        <option value="">{t("All zones")}</option>
        <option value="unmatched">{t("No matching zone")}</option>
        {zoneChoices.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
      </Select></Field></div>
      {domains.isPending ? <p>{t("Loading…")}</p> : null}
      {domains.error ? (
        <p role="alert" className="text-sm break-words text-danger">
          {errorMessage(domains.error, t("Could not load domains"))}
        </p>
      ) : null}
      {domains.data?.length === 0 ? (
        <Empty icon="globe" title={t("No domains yet")} />
      ) : null}
      {domains.data?.length && filtered?.length === 0 ? <Empty icon="globe" title={t("No domains match this zone")} /> : null}
      <div className="min-w-0 overflow-x-auto rounded border border-line">
        <table className="w-full text-left text-sm">
          <thead><tr>{[t("Domain"), t("Zone"), t("Service / stack"), t("Routing mode"), t("DNS check"), t("Actions")].map((label) => <th key={label} scope="col" className="p-3">{label}</th>)}</tr></thead>
          <tbody>{filtered?.map((domain) => (
          <tr className="border-t border-line align-top" key={domain.id ?? `${domain.kind}:${domain.host}`}>
            <td className="max-w-xs break-words p-3">
              <p className="font-medium">{domain.host}{domain.path_prefix !== "/" ? domain.path_prefix : ""}</p>
              <p className="mt-1 text-muted">{domain.ssl_mode ?? t("Unknown")} · {domain.certificate_status ? t(domain.certificate_status) : t("Unknown")}</p>
            </td>
            <td className="max-w-xs break-words p-3">
              {domain.zone_name ?? "—"}
              {domain.routing_mode === "cloudflare" && domain.zone_error ? <p className="mt-1 text-danger">{domain.zone_error}</p> : null}
            </td>
            <td className="max-w-xs break-words p-3">
                {domain.service_id ? (
                  <Link
                    className="underline"
                    to={`/services/${domain.service_id}?tab=domains`}
                  >
                    {domain.service_name} · {domain.service_kind}
                  </Link>
                ) : (
                  t(domain.kind === "panel" ? "Panel" : "Access portal")
                )}
              <p className="mt-1 text-muted">{domain.server_name ?? t("Unknown")}</p>
            </td>
            <td className="max-w-xs break-words p-3">
                <Badge value={domain.routing} />
              {domain.routing_mode === "direct" && domain.routing_options?.direct_warning ? <p className="mt-2 text-amber">{t(domain.routing_options.direct_warning)}</p> : null}
            </td>
            <td className="max-w-sm space-y-1 break-words p-3">
                <p>{t(domain.dns.message)}</p>
                {domain.dns.expected ? (
                  <p>
                    {t("Expected target")}: <code>{domain.dns.expected}</code>
                  </p>
                ) : null}
                <p className="font-mono text-xs">
                  {domain.dns.addresses.join(", ") || t("No addresses")}
                </p>
            </td>
            <td className="p-3">
            {domain.id ? (
              <div className="flex flex-wrap gap-2">
                <Button quiet onClick={() => setEditing(domain)}>
                  {t("Change routing")}
                </Button>
                <Button
                  variant="ghost"
                  disabled={remove.isPending}
                  onClick={() => {
                    if (
                      window.confirm(
                        t("Delete domain {host}?", { host: domain.host }),
                      )
                    )
                      remove.mutate(domain.id!);
                  }}
                >
                  {t("Delete")}
                </Button>
              </div>
            ) : (
              <p className="text-sm text-muted">
                {t("Manage this hostname in its panel or Access settings.")}
              </p>
            )}
            </td>
          </tr>
        ))}</tbody>
        </table>
      </div>
      {editing ? (
        <RoutingDialog domain={editing} onClose={() => setEditing(null)} />
      ) : null}
    </>
  );
}

function RoutingDialog({
  domain,
  onClose,
}: {
  domain: Domain;
  onClose: () => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const [mode, setMode] = useState(domain.routing_mode);
  const [overwrite, setOverwrite] = useState(false);
  const save = useMutation({
    mutationFn: () =>
      api(`/api/v1/domains/${domain.id}`, {
        method: "PATCH",
        body: JSON.stringify({ routing_mode: mode, overwrite_dns: overwrite }),
      }),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ["global-domains"] });
      await client.invalidateQueries({ queryKey: ["domains"] });
      toast.success(t("Domain updated"));
      onClose();
    },
    onError: (error) =>
      toast.error(errorMessage(error, t("Could not update the domain"))),
  });
  return (
    <Dialog
      open
      onClose={onClose}
      title={t("Change routing")}
      description={domain.host}
    >
      <form
        className="space-y-5"
        onSubmit={(event) => {
          event.preventDefault();
          save.mutate();
        }}
      >
        <DomainRouting
          serviceId={domain.service_id!}
          host={domain.host}
          value={mode}
          onChange={setMode}
          overwrite={overwrite}
          onOverwrite={setOverwrite}
        />
        <div className="flex flex-wrap justify-end gap-2">
          <Button type="button" quiet onClick={onClose}>
            {t("Cancel")}
          </Button>
          <Button type="submit" disabled={save.isPending}>
            {t("Save")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

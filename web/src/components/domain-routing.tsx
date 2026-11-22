import { useQuery } from "@tanstack/react-query";
import { api } from "../api";
import type { Schemas } from "../types";
import { useT } from "../i18n";
import { Field, Select } from "./ui";

export function DomainRouting({
  serviceId,
  host,
  value,
  onChange,
  overwrite,
  onOverwrite,
}: {
  serviceId: string;
  host: string;
  value: string;
  onChange: (value: string) => void;
  overwrite: boolean;
  onOverwrite: (value: boolean) => void;
}) {
  const t = useT();
  const options = useQuery({
    queryKey: ["domain-routing-options", serviceId, host.trim().toLowerCase()],
    queryFn: () =>
      api<Schemas["DomainRoutingOptions"]>(
        `/api/v1/domains/routing-options?service_id=${serviceId}&host=${encodeURIComponent(host.trim().toLowerCase())}`,
      ),
    enabled: Boolean(host.trim()),
  });
  const available = options.data?.cloudflare_available ?? false;
  const reason =
    options.data?.cloudflare_reason_code === "zone"
      ? t("The hostname must be inside {zone}. Wildcards are not supported.", {
          zone: options.data.zone_name,
        })
      : options.data?.cloudflare_reason
        ? t(options.data.cloudflare_reason)
        : undefined;
  return (
    <div className="min-w-0 space-y-3">
      <Field label={t("Routing mode")} hint={reason}>
        <Select
          value={value}
          onChange={(event) => onChange(event.target.value)}
        >
          <option value="direct">{t("Direct")}</option>
          <option value="cloudflare" disabled={!available}>
            {t("Cloudflare (tunnel or proxied DNS)")}
          </option>
        </Select>
      </Field>
      {options.isError ? (
        <p role="alert" className="text-sm text-danger">
          {t("Could not check routing availability. Try again.")}
        </p>
      ) : null}
      {!host.trim() ? (
        <p className="text-sm text-muted">
          {t("Enter a hostname to check Cloudflare availability.")}
        </p>
      ) : null}
      {value === "direct" && options.data?.direct_warning ? (
        <p
          role="status"
          className="rounded-alert border border-line bg-paper p-3 text-sm text-amber"
        >
          {t(options.data.direct_warning)}
        </p>
      ) : null}
      {value === "cloudflare" ? (
        <label className="flex min-h-11 items-start gap-2 text-sm">
          <input
            type="checkbox"
            className="mt-1"
            checked={overwrite}
            onChange={(event) => onOverwrite(event.target.checked)}
          />
          {t(
            "Replace conflicting DNS records (including records not created by Rudder)",
          )}
        </label>
      ) : null}
    </div>
  );
}

import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";
import { api } from "../api";
import { errorMessage } from "../format";
import { useT } from "../i18n";
import type { Schemas } from "../types";
import { Field, Select, TextArea, TextInput } from "./ui";

export const DEFAULT_BUILD_SETTINGS: Schemas["ServiceBuildSettings"] = {
  placement: "panel",
  builder_id: null,
  fallback: "panel",
  test_command: null,
  timeout_secs: 3600,
};

export function BuildSettingsFields({
  value,
  onChange,
}: {
  value: Schemas["ServiceBuildSettings"];
  onChange: (value: Schemas["ServiceBuildSettings"]) => void;
}) {
  const t = useT();
  const inventory = useQuery({
    queryKey: ["builders"],
    queryFn: () => api<Schemas["BuilderInventory"]>("/api/v1/builders"),
    refetchInterval: 5000,
  });
  const builders = inventory.data?.builders ?? [];
  const available = builders.filter((builder) => builder.enabled);
  const change = (next: Partial<Schemas["ServiceBuildSettings"]>) =>
    onChange({ ...value, ...next });
  return (
    <fieldset className="grid gap-4 rounded-md border border-line p-4 md:col-span-2 md:grid-cols-2">
      <legend className="px-1 text-sm font-semibold">
        {t("Build host settings")}
      </legend>
      <Field
        label={t("Build placement")}
        hint={t(
          "Panel keeps today's behavior. Auto ranks compatible hosts by cache, free slots and name.",
        )}
      >
        <Select
          value={value.placement}
          onChange={(event) =>
            change({ placement: event.target.value, builder_id: null })
          }
        >
          <option value="panel">{t("Panel")}</option>
          <option value="auto">{t("Automatic build host")}</option>
          <option value="builder">{t("Specific build host")}</option>
        </Select>
      </Field>
      <Field
        label={t("Build host")}
        hint={t(
          "A specific host must be enabled. It also needs a matching architecture, Buildx and any required Nixpacks capability.",
        )}
      >
        <Select
          value={value.builder_id ?? ""}
          disabled={value.placement !== "builder" || available.length === 0}
          onChange={(event) =>
            change({ builder_id: event.target.value || null })
          }
        >
          <option value="">{t("Choose a build host")}</option>
          {builders
            .filter(
              (builder) => builder.enabled || builder.id === value.builder_id,
            )
            .map((builder) => (
              <option
                key={builder.id}
                value={builder.id}
                disabled={!builder.enabled}
              >
                {builder.name}
                {!builder.enabled ? ` · ${t("Builder disabled")}` : ""}
              </option>
            ))}
          {value.builder_id &&
          !builders.some((builder) => builder.id === value.builder_id) ? (
            <option value={value.builder_id}>
              {t("Selected build host is unavailable")}
            </option>
          ) : null}
        </Select>
      </Field>
      {inventory.isError ? (
        <p role="alert" className="text-sm text-danger md:col-span-2">
          {errorMessage(inventory.error, t("Could not load build hosts"))}
        </p>
      ) : null}
      {!inventory.isLoading && !inventory.isError && available.length === 0 ? (
        <p className="text-sm text-muted md:col-span-2">
          {t("No enabled build hosts.")}{" "}
          <Link className="underline" to="/builders">
            {t("Add build host")}
          </Link>
        </p>
      ) : null}
      <Field
        label={t("Build fallback")}
        hint={t(
          "If no host is eligible, build on the panel or leave the job queued until a host becomes available.",
        )}
      >
        <Select
          value={value.fallback}
          disabled={value.placement === "panel"}
          onChange={(event) => change({ fallback: event.target.value })}
        >
          <option value="panel">{t("Build on panel")}</option>
          <option value="wait">{t("Wait for a build host")}</option>
        </Select>
      </Field>
      <Field
        label={t("Build timeout (seconds)")}
        hint={t(
          "A positive deadline shared by building and the optional image test; default 3600 seconds.",
        )}
      >
        <TextInput
          type="number"
          min={1}
          max={2147483647}
          required
          value={value.timeout_secs}
          onChange={(event) =>
            change({ timeout_secs: Number(event.target.value) })
          }
        />
      </Field>
      <div className="md:col-span-2">
        <Field
          label={t("Image test command")}
          hint={t(
            "Optional shell command inside the built image, using Docker's default network. A nonzero exit stops deployment on the panel or a builder.",
          )}
        >
          <TextArea
            value={value.test_command ?? ""}
            maxLength={8192}
            placeholder="npm test"
            onChange={(event) =>
              change({ test_command: event.target.value || null })
            }
          />
        </Field>
      </div>
    </fieldset>
  );
}

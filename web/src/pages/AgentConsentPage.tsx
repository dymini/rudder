import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useSearchParams } from "react-router-dom";
import { toast } from "sonner";
import { api, ApiError } from "../api";
import { Button, Card, Field, TextInput } from "../components/ui";
import { useT } from "../i18n";
import { McpPermissions, type PermissionCatalog } from "../components/mcp-permissions";

interface Scope {
  id: string;
  group: string;
  write: boolean;
  summary: string;
}

interface Template {
  id: string;
  name: string;
  summary: string;
  scopes: string[];
}

interface Catalog {
  scopes: Scope[];
  templates: Template[];
  tools: PermissionCatalog["tools"];
}

interface ClientView {
  client_id: string;
  client_name: string;
  redirect_uris: string[];
}

function templateCopy(t: (key: string) => string, id: string): { name: string; summary: string } {
  switch (id) {
    case "full":
      return {
        name: t("Full control"),
        summary: t("Read and change every setting and all data."),
      };
    case "read-all":
      return {
        name: t("Read everything"),
        summary: t("Read every setting and all data. Cannot change anything."),
      };
    case "operator":
      return {
        name: t("Operate"),
        summary: t(
          "Deploy and manage apps, Access, security, caches, logs, and monitors. Cannot change users, tokens, or panel updates.",
        ),
      };
    case "observer":
      return {
        name: t("Watch"),
        summary: t("Read health, logs, metrics, cache stats, and security checks."),
      };
    default:
      return { name: t("Custom"), summary: t("Choose each area this agent may read or change.") };
  }
}

export function AgentConsentPage() {
  const t = useT();
  const [params] = useSearchParams();
  const clientId = params.get("client_id") ?? "";
  const redirectUri = params.get("redirect_uri") ?? "";
  const challenge = params.get("code_challenge") ?? "";
  const method = params.get("code_challenge_method") ?? "";
  const state = params.get("state") ?? "";
  const resource = params.get("resource");
  const catalog = useQuery({
    queryKey: ["mcp-catalog"],
    queryFn: () => api<Catalog>("/api/v1/mcp/catalog"),
  });
  const client = useQuery({
    queryKey: ["mcp-client", clientId],
    queryFn: () => api<ClientView>(`/api/v1/mcp/clients/${encodeURIComponent(clientId)}`),
    enabled: clientId.length > 0,
  });
  const [template, setTemplate] = useState("full");
  const [custom, setCustom] = useState<string[]>([]);
  const [tools, setTools] = useState<string[]>([]);
  const [name, setName] = useState("");

  const approve = useMutation({
    mutationFn: () =>
      api<{ redirect: string }>("/api/v1/mcp/consent", {
        method: "POST",
        body: JSON.stringify({
          client_id: clientId,
          redirect_uri: redirectUri,
          code_challenge: challenge,
          code_challenge_method: method,
          state,
          resource,
          template: template === "custom" ? "custom" : template,
          scopes: template === "custom" ? custom : [],
          allowed_tools: template === "custom" ? tools : [],
          name,
        }),
      }),
    onSuccess: (body) => {
      window.location.assign(body.redirect);
    },
    onError: (error) =>
      toast.error(error instanceof ApiError ? error.message : t("Could not approve the agent")),
  });

  const missing = !clientId || !redirectUri || !challenge || !state;

  return (
    <>
      <div>
        <h1 className="text-2xl font-semibold">{t("Approve an agent")}</h1>
        <p className="text-sm text-muted">
          {client.data
            ? t("{name} wants to control this panel.", { name: client.data.client_name })
            : t("An agent wants to control this panel.")}
        </p>
      </div>
      {missing ? (
        <Card title={t("Something is missing")}>
          <p className="text-sm text-muted">
            {t("Open this page from the agent. The sign-in request was incomplete.")}
          </p>
        </Card>
      ) : (
        <Card title={t("Permissions")}>
          <p className="mb-4 break-all text-sm text-muted">
            {t("After you approve, the browser returns to {url}.", { url: redirectUri })}
          </p>
          <div className="grid gap-3">
            {(catalog.data?.templates ?? []).map((item) => (
              <label
                key={item.id}
                className={`cursor-pointer rounded-md border p-3 ${template === item.id ? "border-pine bg-pine-soft" : "border-line"}`}
              >
                <span className="flex items-start gap-2">
                  <input
                    type="radio"
                    name="template"
                    className="mt-1"
                    checked={template === item.id}
                    onChange={() => setTemplate(item.id)}
                  />
                  <span>
                    <span className="block font-medium">{templateCopy(t, item.id).name}</span>
                    <span className="block text-sm text-muted">{templateCopy(t, item.id).summary}</span>
                  </span>
                </span>
              </label>
            ))}
            <label
              className={`cursor-pointer rounded-md border p-3 ${template === "custom" ? "border-pine bg-pine-soft" : "border-line"}`}
            >
              <span className="flex items-start gap-2">
                <input
                  type="radio"
                  name="template"
                  className="mt-1"
                  checked={template === "custom"}
                  onChange={() => setTemplate("custom")}
                />
                <span>
                  <span className="block font-medium">{t("Custom")}</span>
                  <span className="block text-sm text-muted">
                    {t("Choose each area this agent may read or change.")}
                  </span>
                </span>
              </span>
            </label>
          </div>
          {template === "custom" && catalog.data ? <McpPermissions catalog={catalog.data} scopes={custom} tools={tools} onChange={(scopes, tools) => { setCustom(scopes); setTools(tools); }} /> : null}
          <form
            className="mt-4 grid gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              approve.mutate();
            }}
          >
            <Field label={t("Name")} hint={t("Shown in the list of approved agents. Optional.")}>
              <TextInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={client.data?.client_name ?? ""}
              />
            </Field>
            <div>
              <Button type="submit" disabled={approve.isPending}>
                {approve.isPending ? t("Approving…") : t("Approve")}
              </Button>
            </div>
          </form>
        </Card>
      )}
    </>
  );
}

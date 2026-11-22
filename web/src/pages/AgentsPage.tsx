import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { api, ApiError } from "../api";
import { Button, Card, Empty } from "../components/ui";
import { formatRelative } from "../format";
import { useT } from "../i18n";
import { McpPermissions, type PermissionCatalog } from "../components/mcp-permissions";

interface Grant {
  id: string;
  name: string;
  client_name: string;
  template: string;
  scopes: string[];
  allowed_tools: string[] | null;
  client_id: string;
  created_at: string;
  last_used_at: string | null;
}

function templateLabel(t: (key: string) => string, id: string): string {
  switch (id) {
    case "full":
      return t("Full control");
    case "read-all":
      return t("Read everything");
    case "operator":
      return t("Operate");
    case "observer":
      return t("Watch");
    default:
      return t("Custom");
  }
}

export function AgentsPage() {
  const t = useT();
  const queryClient = useQueryClient();
  const grants = useQuery({
    queryKey: ["mcp-grants"],
    queryFn: () => api<Grant[]>("/api/v1/mcp/grants"),
  });
  const address = `${window.location.origin}/mcp`;
  const [copied, setCopied] = useState(false);
  const catalog = useQuery({ queryKey: ["mcp-catalog"], queryFn: () => api<PermissionCatalog>("/api/v1/mcp/catalog") });
  const [editing, setEditing] = useState<Grant | null>(null);
  const [template, setTemplate] = useState("custom");
  const [scopes, setScopes] = useState<string[]>([]);
  const [tools, setTools] = useState<string[]>([]);
  const save = useMutation({
    mutationFn: () => api(`/api/v1/mcp/grants/${editing?.id}`, { method: "PUT", body: JSON.stringify({ template, scopes: template === "custom" ? scopes : [], allowed_tools: template === "custom" ? tools : [] }) }),
    onSuccess: async () => { setEditing(null); toast.success(t("Agent permissions saved")); await queryClient.invalidateQueries({ queryKey: ["mcp-grants"] }); },
    onError: (error) => toast.error(error.message),
  });

  const revoke = useMutation({
    mutationFn: (id: string) => api(`/api/v1/mcp/grants/${id}`, { method: "DELETE" }),
    onSuccess: async () => {
      toast.success(t("Agent revoked"));
      await queryClient.invalidateQueries({ queryKey: ["mcp-grants"] });
    },
    onError: (error) =>
      toast.error(error instanceof ApiError ? error.message : t("Could not revoke the agent")),
  });

  return (
    <>
      <div>
        <h1 className="text-2xl font-semibold">{t("AI agents")}</h1>
        <p className="text-sm text-muted">
          {t(
            "An agent connects to this address and asks you to approve what it may do. Full control can read and change every setting and all data.",
          )}
        </p>
      </div>
      <Card title={t("Connection address")}>
        <p className="mb-3 text-sm text-muted">
          {t(
            "In the agent, add a remote MCP server with this address. You will sign in and pick a permission template.",
          )}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <code className="break-all rounded-md bg-paper px-3 py-2 font-mono text-sm">{address}</code>
          <Button
            quiet
            type="button"
            onClick={() => {
              void navigator.clipboard.writeText(address).then(() => {
                setCopied(true);
                toast.success(t("Address copied"));
              });
            }}
          >
            {copied ? t("Copied") : t("Copy")}
          </Button>
        </div>
      </Card>
      <Card title={t("Approved agents")}>
        {grants.isPending ? <p role="status">{t("Loading…")}</p> : null}
        {grants.isError ? <p role="alert">{grants.error.message}</p> : null}
        {grants.data?.length === 0 ? (
          <Empty>{t("No agents yet. Connect one with the address above.")}</Empty>
        ) : null}
        <ul className="divide-y divide-line">
          {(grants.data ?? []).map((grant) => (
            <li key={grant.id} className="flex flex-wrap items-center gap-3 py-3">
              <div className="min-w-0 flex-1">
                <p className="font-medium">{grant.name}</p>
                <p className="text-sm text-muted">
                  {templateLabel(t, grant.template)} · {grant.client_name} ·{" "}
                  {grant.last_used_at
                    ? t("Used {time}", { time: formatRelative(grant.last_used_at) })
                    : t("Not used yet")}
                </p>
              </div>
              <Button
                quiet
                onClick={() => { setEditing(grant); setTemplate(grant.template); setScopes(grant.scopes); setTools(grant.allowed_tools ?? []); }}
              >{t("Edit permissions")}</Button>
              <Button
                quiet
                type="button"
                onClick={() => {
                  if (window.confirm(t("Revoke {name}? It will have to be approved again.", { name: grant.name }))) {
                    revoke.mutate(grant.id);
                  }
                }}
              >
                {t("Revoke")}
              </Button>
            </li>
          ))}
        </ul>
      </Card>
      {editing ? <Card title={t("Edit agent permissions")}>
        <p className="mb-3 break-words">{editing.name}</p>
        {catalog.isError ? <p role="alert">{catalog.error.message}</p> : null}
        <label className="grid gap-1 text-sm">{t("Permission template")}
          <select className="min-h-10 rounded-md border border-line bg-paper px-3" value={template} onChange={(event) => setTemplate(event.target.value)}>
            {(catalog.data?.templates ?? []).map((item) => <option key={item.id} value={item.id}>{templateLabel(t, item.id)}</option>)}
            <option value="custom">{t("Custom")}</option>
          </select>
        </label>
        {template === "custom" && catalog.data ? <McpPermissions catalog={catalog.data} scopes={scopes} tools={tools} onChange={(scopes, tools) => { setScopes(scopes); setTools(tools); }} /> : null}
        <p className="my-3 text-sm text-muted">{t("Changes apply immediately to this agent and its delegated API keys.")}</p>
        <div className="flex flex-wrap gap-2"><Button disabled={save.isPending || !catalog.data} onClick={() => save.mutate()}>{t("Save permissions")}</Button><Button quiet onClick={() => setEditing(null)}>{t("Cancel")}</Button></div>
      </Card> : null}
    </>
  );
}

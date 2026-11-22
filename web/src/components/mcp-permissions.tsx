import { useT } from "../i18n";

export interface PermissionCatalog {
  scopes: { id: string; group: string; write: boolean; summary: string }[];
  tools: { name: string; scope: string; description: string }[];
  templates: { id: string; name: string; summary: string; scopes: string[] }[];
}

export function McpPermissions({
  catalog,
  scopes,
  tools,
  onChange,
}: {
  catalog: PermissionCatalog;
  scopes: string[];
  tools: string[];
  onChange: (scopes: string[], tools: string[]) => void;
}) {
  const t = useT();
  const groups = [...new Set(catalog.scopes.map((scope) => scope.group))];
  return (
    <div className="mt-4 min-w-0 space-y-3">
      <p className="text-sm text-muted">
        {t(
          "Group permissions include every tool in that scope. Clear a group permission to choose its tools individually.",
        )}
      </p>
      {groups.map((group) => {
        const groupScopes = catalog.scopes.filter(
          (scope) => scope.group === group,
        );
        const groupTools = catalog.tools.filter((tool) =>
          tool.scope.startsWith(`${group}:`),
        );
        return (
          <details
            key={group}
            className="min-w-0 rounded-md border border-line p-3"
          >
            <summary className="cursor-pointer break-words py-1 font-medium">
              {t(groupScopes[0]?.summary ?? group)}{" "}
              <span className="text-xs text-muted">({groupTools.length})</span>
            </summary>
            <div className="my-3 flex flex-wrap gap-4 text-sm">
              {groupScopes.map((scope) => (
                <label
                  key={scope.id}
                  className="flex min-h-9 items-center gap-2"
                >
                  <input
                    type="checkbox"
                    checked={scopes.includes(scope.id)}
                    onChange={(event) => {
                      const next = new Set(scopes);
                      if (event.target.checked) {
                        next.add(scope.id);
                        if (scope.write)
                          next.add(scope.id.replace(":write", ":read"));
                      } else {
                        next.delete(scope.id);
                        if (!scope.write)
                          next.delete(scope.id.replace(":read", ":write"));
                      }
                      onChange([...next], tools);
                    }}
                  />
                  {scope.write
                    ? t("Change entire group")
                    : t("Read entire group")}
                </label>
              ))}
            </div>
            <ul className="max-h-80 space-y-1 overflow-auto">
              {groupTools.map((tool) => {
                const grouped = scopes.includes(tool.scope);
                return (
                  <li key={tool.name}>
                    <label className="flex min-h-10 items-start gap-2 py-2 text-sm">
                      <input
                        className="mt-1"
                        type="checkbox"
                        disabled={grouped}
                        checked={grouped || tools.includes(tool.name)}
                        onChange={(event) =>
                          onChange(
                            scopes,
                            event.target.checked
                              ? [...tools, tool.name]
                              : tools.filter((name) => name !== tool.name),
                          )
                        }
                      />
                      <span className="min-w-0">
                        <code className="break-all text-xs">{tool.name}</code>
                        <span className="block break-words text-muted">
                          {tool.description}
                        </span>
                      </span>
                    </label>
                  </li>
                );
              })}
            </ul>
          </details>
        );
      })}
    </div>
  );
}

export const nameRules = {
  resource: {
    pattern: "[a-z0-9][a-z0-9\\-]{0,62}",
    message: "Name must be 1 to 63 characters: lowercase ASCII letters (a-z), digits (0-9), or hyphens (-), starting with a letter or digit.",
  },
  compose: {
    pattern: "[a-zA-Z0-9][a-zA-Z0-9._\\-]{0,62}",
    message: "Compose service name must be 1 to 63 ASCII letters, digits, dots, underscores, or hyphens, starting with a letter or digit.",
  },
  environment: {
    pattern: "[a-zA-Z_][a-zA-Z0-9_]{0,127}",
    message: "Environment variable name must be 1 to 128 ASCII letters, digits, or underscores, starting with a letter or underscore.",
  },
} as const;

export type NameKind = keyof typeof nameRules;

export function nameError(value: string, kind: NameKind = "resource", required = true): string | null {
  if (!value && !required) return null;
  const rule = nameRules[kind];
  return new RegExp(`^(?:${rule.pattern})$`, "u").exec(value)?.[0] === value ? null : rule.message;
}

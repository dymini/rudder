export const colors = {
  bg: "#0b0f17",
  card: "#151b26",
  cardBorder: "#232c3d",
  text: "#e8edf5",
  textDim: "#8b98ad",
  accent: "#4f8ef7",
  accentDim: "#1d3252",
  ok: "#3fb96f",
  warn: "#e0a83c",
  danger: "#e05c5c",
  busy: "#b07ce0",
};

export type Tone = "ok" | "warn" | "danger" | "busy" | "dim";

export function statusTone(status: string | null | undefined): Tone {
  const value = (status ?? "").toLowerCase();
  if (["running", "healthy", "ok", "success", "online", "up", "active"].some((k) => value.includes(k))) return "ok";
  if (["building", "deploying", "pending", "starting", "pulling", "queued"].some((k) => value.includes(k))) return "busy";
  if (["error", "failed", "down", "unhealthy", "exited", "dead"].some((k) => value.includes(k))) return "danger";
  if (["stopped", "paused", "warn"].some((k) => value.includes(k))) return "warn";
  return "dim";
}

export const spacing = {
  xs: 4,
  sm: 8,
  md: 12,
  lg: 16,
  xl: 24,
};

import { Platform } from "react-native";

export const colors = {
  bg: "#050812",
  bgElevated: "#0a1020",
  card: "#101827",
  cardSoft: "#162033",
  cardBorder: "#263449",
  text: "#f4f7fb",
  textDim: "#93a1b7",
  textFaint: "#64748b",
  accent: "#61a5ff",
  accentBright: "#7dd3fc",
  accentDim: "#173759",
  ok: "#34d399",
  warn: "#fbbf24",
  danger: "#fb7185",
  busy: "#a78bfa",
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
  xxl: 32,
};

export const radius = {
  sm: 10,
  md: 14,
  lg: 20,
  xl: 28,
};

export const monoFont = Platform.OS === "ios" ? "Menlo" : "monospace";

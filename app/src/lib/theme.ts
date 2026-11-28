import * as Haptics from "expo-haptics";
import { Platform, StyleSheet } from "react-native";

/**
 * Dymini Panel mobile ledger — unified with the web console's Warm Obsidian + Sage Pine identity.
 * Deliberately calm, high-contrast, and free of neon blue/purple clichés.
 */
export const colors = {
  bg: "#0d0c0b",
  bgElevated: "#141311",
  card: "#171614",
  cardSoft: "#211f1c",
  cardBorder: "#292622",
  cardBorderStrong: "#38342e",
  text: "#f3efe8",
  textSoft: "#d6d0c6",
  textDim: "#9c958b",
  textFaint: "#6b655d",
  accent: "#7ec4a4",
  accentBright: "#96d4b7",
  accentInk: "#0a1410",
  accentDim: "#172a22",
  accentBorder: "#274437",
  ok: "#7ec4a4",
  warn: "#e2b564",
  warnDim: "#261e11",
  danger: "#e6867c",
  dangerDim: "#281615",
  busy: "#78a6cf",
};

export type Tone = "ok" | "warn" | "danger" | "busy" | "dim";

export function statusTone(status: string | null | undefined): Tone {
  const value = (status ?? "").toLowerCase();
  if (["running", "healthy", "ok", "success", "online", "up", "active"].some((k) => value.includes(k))) return "ok";
  if (["building", "deploying", "pending", "starting", "pulling", "queued"].some((k) => value.includes(k))) return "busy";
  if (["error", "failed", "down", "unhealthy", "exited", "dead", "firing", "critical"].some((k) => value.includes(k))) return "danger";
  if (["stopped", "paused", "warn", "busy"].some((k) => value.includes(k))) return "warn";
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
  sm: 8,
  md: 12,
  lg: 14,
  xl: 18,
};

export const hairline = StyleSheet.hairlineWidth;

export const monoFont = Platform.OS === "ios" ? "Menlo" : "monospace";

export function tapHaptic(kind: "select" | "light" | "medium" = "select") {
  if (Platform.OS === "web") return;
  try {
    if (kind === "select") {
      void Haptics.selectionAsync();
    } else if (kind === "light") {
      void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
    } else {
      void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium);
    }
  } catch {
    // Ignore on unsupported devices.
  }
}

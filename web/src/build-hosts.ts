export function cachePercent(bytes: number, limit: number): number {
  if (!Number.isFinite(bytes) || !Number.isFinite(limit) || bytes <= 0)
    return 0;
  return limit <= 0 ? 100 : Math.min(100, (bytes / limit) * 100);
}

export function waitingReason(
  status: string,
  logs: string | null | undefined,
): string | null {
  if (status !== "queued" || !logs) return null;
  return (
    logs
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.startsWith("Waiting:"))
      .at(-1) ?? null
  );
}

import { Link } from "react-router-dom";
import { formatBytes, formatRelative } from "../../format";
import { useT } from "../../i18n";
import type { Schemas } from "../../types";
import { Icon } from "../icons";
import { ThresholdMeter } from "../load";
import { Badge, Card, Empty, Skeleton, StatusDot } from "../ui";

type Server = Schemas["ServerView"];
type Usage = Schemas["ServerUsage"];

/**
 * Interactive Fleet Matrix card.
 * Clicking a server selects it to cross-filter the TelemetryPanel chart, marked with a clean left-edge indicator.
 */
export function FleetPanel({
  servers,
  usage,
  loading,
  selectedServerId,
  onSelectServer,
}: {
  servers: Server[];
  usage: Map<string, Usage>;
  loading: boolean;
  selectedServerId: string | null;
  onSelectServer: (id: string) => void;
}) {
  const t = useT();
  const activeId = selectedServerId ?? servers[0]?.id ?? null;

  return (
    <Card
      title={t("Server fleet")}
      flush
      actions={
        <Link
          to="/servers"
          className="inline-flex items-center gap-1 text-xs font-medium text-muted transition-colors hover:text-ink"
        >
          {t("Manage")}
          <Icon name="arrowRight" size={13} />
        </Link>
      }
    >
      {loading ? (
        <div className="space-y-3 px-4 pb-4 sm:px-5">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
        </div>
      ) : servers.length === 0 ? (
        <Empty icon="server" title={t("No servers yet")}>
          {t("{link} to start deploying.", { link: <Link to="/servers">{t("Add a server")}</Link> })}
        </Empty>
      ) : (
        <ul className="divide-y divide-line border-t border-line">
          {servers.map((server) => {
            const row = usage.get(server.id);
            const selected = server.id === activeId;
            return (
              <li key={server.id}>
                <button
                  type="button"
                  onClick={() => onSelectServer(server.id)}
                  aria-pressed={selected}
                  className={`relative block w-full px-4 py-3.5 text-left transition-colors sm:px-5 ${
                    selected
                      ? "bg-paper/80 before:absolute before:top-0 before:bottom-0 before:left-0 before:w-[2.5px] before:bg-pine"
                      : "hover:bg-paper/50"
                  }`}
                >
                  <div className="flex items-start justify-between gap-2.5">
                    <div className="min-w-0">
                      <p className="flex items-center gap-2 text-sm font-medium text-ink">
                        <StatusDot status={server.status} />
                        <span className="truncate">{server.name}</span>
                        {server.kind === "local" ? (
                          <span className="font-mono text-[10.5px] text-faint">{t("panel")}</span>
                        ) : null}
                      </p>
                      <p className="mt-0.5 truncate pl-4 font-mono text-[11px] text-faint">
                        {[server.public_ip, server.hostname].filter(Boolean).join(" · ") ||
                          server.kind}
                      </p>
                    </div>
                    <Badge value={server.status} />
                  </div>

                  {row && server.status === "online" ? (
                    <dl className="mt-2.5 grid grid-cols-3 gap-3 pl-4">
                      <MiniGauge label={t("CPU")} value={row.cpu_percent} />
                      <MiniGauge label={t("RAM")} value={row.memory_percent} />
                      <MiniGauge label={t("Disk")} value={row.disk_percent} />
                    </dl>
                  ) : (
                    <p className="mt-2 pl-4 text-xs text-muted">
                      {server.last_heartbeat_at
                        ? t("Last seen {time}", { time: formatRelative(server.last_heartbeat_at) })
                        : server.memory_bytes
                          ? `${server.cpu_count ?? "?"} vCPU · ${formatBytes(server.memory_bytes)}`
                          : t("No samples yet")}
                    </p>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}

function MiniGauge({ label, value }: { label: string; value: number | null | undefined }) {
  const t = useT();
  return (
    <div className="min-w-0">
      <div className="flex items-baseline justify-between gap-1 text-[11px]">
        <dt className="text-muted">{label}</dt>
        <dd className="font-mono font-medium text-ink-soft tabular-nums">
          {value == null ? "—" : `${Math.round(value)}%`}
        </dd>
      </div>
      <div className="mt-1">
        <ThresholdMeter
          value={value}
          warn={75}
          danger={90}
          marks={false}
          label={t("{label} usage", { label })}
        />
      </div>
    </div>
  );
}

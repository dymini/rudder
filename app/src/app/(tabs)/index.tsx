import { Ionicons } from "@expo/vector-icons";
import { useQuery } from "@tanstack/react-query";
import { router } from "expo-router";
import { Pressable, StyleSheet, Text, View } from "react-native";
import {
  Badge,
  Empty,
  ErrorBox,
  GroupedRow,
  HairlineBar,
  InsetGroup,
  Loading,
  MetricLedger,
  Screen,
  SectionTitle,
} from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { colors, monoFont, radius, spacing, statusTone, tapHaptic } from "../../lib/theme";
import {
  timeAgo,
  type EventView,
  type MetricsOverview,
  type ServerView,
  type ServiceSummary,
  type SystemInfo,
} from "../../lib/types";

function MiniMeter({ label, value }: { label: string; value: number }) {
  const pct = Math.max(0, Math.min(100, value));
  return (
    <View style={styles.miniMeter}>
      <View style={styles.miniMeterHead}>
        <Text style={styles.miniMeterLabel}>{label}</Text>
        <Text style={styles.miniMeterVal}>{Math.round(pct)}%</Text>
      </View>
      <HairlineBar value={pct} />
    </View>
  );
}

const QUICK_NAV: { label: string; icon: keyof typeof Ionicons.glyphMap; route: string }[] = [
  { label: "Tải hệ thống", icon: "speedometer-outline", route: "/load" },
  { label: "Thống kê", icon: "stats-chart-outline", route: "/statistics" },
  { label: "Nhật ký", icon: "terminal-outline", route: "/logs" },
  { label: "Máy chủ", icon: "server-outline", route: "/servers" },
];

export default function Dashboard() {
  const { api } = useAuth();
  const system = useQuery({ queryKey: ["system"], queryFn: () => api<SystemInfo>("/api/v1/system") });
  const services = useQuery({ queryKey: ["services"], queryFn: () => api<ServiceSummary[]>("/api/v1/services") });
  const servers = useQuery({ queryKey: ["servers"], queryFn: () => api<ServerView[]>("/api/v1/servers") });
  const metrics = useQuery({ queryKey: ["metrics-overview"], queryFn: () => api<MetricsOverview>("/api/v1/metrics/overview") });
  const events = useQuery({ queryKey: ["events"], queryFn: () => api<EventView[]>("/api/v1/events?limit=6") });

  function refetch() {
    system.refetch();
    services.refetch();
    servers.refetch();
    metrics.refetch();
    events.refetch();
  }

  if (system.isLoading) {
    return (
      <Screen scroll={false}>
        <Loading />
      </Screen>
    );
  }

  const list = services.data ?? [];
  const serverList = servers.data ?? [];
  const running = list.filter((s) => (s.status ?? "").toLowerCase().includes("running")).length;
  const trouble = list.filter((s) => {
    const v = (s.status ?? "").toLowerCase();
    return v.includes("error") || v.includes("fail") || v.includes("exited");
  });
  const online = serverList.filter((s) => (s.status ?? "").toLowerCase().includes("online")).length;
  const offlineCount = serverList.length - online;
  const incidentCount = trouble.length + offlineCount;
  const healthy = incidentCount === 0;

  return (
    <Screen refreshing={system.isRefetching} onRefresh={refetch}>
      <ErrorBox error={system.error ?? services.error} />

      {/* Quiet Operational Status Header */}
      <View style={styles.statusHeader}>
        <View style={styles.statusLeft}>
          <View
            style={[
              styles.statusDot,
              { backgroundColor: healthy ? colors.ok : colors.danger },
            ]}
          />
          <Text style={styles.statusText} numberOfLines={1}>
            {healthy
              ? "Mọi hệ thống ổn định"
              : `${incidentCount} vấn đề cần kiểm tra`}
          </Text>
        </View>
        {system.data?.version ? (
          <Text style={styles.versionTag}>v{system.data.version}</Text>
        ) : null}
      </View>

      {/* Hairline-divided 3-Column Metric Ledger */}
      <MetricLedger
        items={[
          {
            label: "Dịch vụ",
            value: String(running),
            suffix: `/ ${list.length}`,
            caption: list.length - running > 0 ? `${list.length - running} dừng` : "đang chạy",
            tone: trouble.length > 0 ? "danger" : "ok",
            onPress: () => router.push("/(tabs)/services"),
          },
          {
            label: "Máy chủ",
            value: String(online),
            suffix: `/ ${serverList.length}`,
            caption: offlineCount > 0 ? `${offlineCount} ngoại tuyến` : "trực tuyến",
            tone: offlineCount > 0 ? "danger" : "ok",
            onPress: () => router.push("/servers"),
          },
          {
            label: "Sự cố",
            value: String(incidentCount),
            caption: healthy ? "không có" : "cần xử lý",
            tone: healthy ? "dim" : "danger",
            onPress: () => router.push(trouble.length > 0 ? "/(tabs)/services" : "/load"),
          },
        ]}
      />

      {/* Quick Ops Rail */}
      <View style={styles.quickRow}>
        {QUICK_NAV.map((item) => (
          <Pressable
            key={item.route}
            onPress={() => {
              tapHaptic("select");
              router.push(item.route as never);
            }}
            style={({ pressed }) => [
              styles.quickChip,
              pressed && { backgroundColor: colors.cardSoft },
            ]}
          >
            <Ionicons name={item.icon} size={15} color={colors.accent} />
            <Text style={styles.quickLabel} numberOfLines={1}>
              {item.label}
            </Text>
          </Pressable>
        ))}
      </View>

      {/* Priority Attention Queue — only rendered when needed */}
      {trouble.length > 0 ? (
        <>
          <SectionTitle>Cần chú ý</SectionTitle>
          <InsetGroup style={{ borderColor: colors.danger + "44" }}>
            {trouble.map((service) => (
              <GroupedRow
                key={service.id}
                icon="alert-circle-outline"
                tone="danger"
                title={service.name}
                subtitle={`${service.project_name} / ${service.environment_name}`}
                right={<Badge label={service.status} tone={statusTone(service.status)} />}
                onPress={() => router.push(`/service/${service.id}`)}
              />
            ))}
          </InsetGroup>
        </>
      ) : null}

      {/* Fleet Telemetry Matrix */}
      <SectionTitle>Cụm máy chủ</SectionTitle>
      {serverList.length === 0 ? (
        <Empty text="Chưa có máy chủ nào." />
      ) : (
        <InsetGroup>
          {serverList.map((server) => {
            const usage = metrics.data?.servers?.find((s) => s.server_id === server.id);
            const isOnline = (server.status ?? "").toLowerCase().includes("online");
            return (
              <Pressable
                key={server.id}
                onPress={() => {
                  tapHaptic("select");
                  router.push("/servers");
                }}
                style={({ pressed }) => [
                  styles.serverRow,
                  pressed && { backgroundColor: colors.cardSoft },
                ]}
              >
                <View style={styles.serverTop}>
                  <View style={styles.serverIdentity}>
                    <View
                      style={[
                        styles.serverDot,
                        { backgroundColor: isOnline ? colors.ok : colors.danger },
                      ]}
                    />
                    <View style={{ flex: 1, minWidth: 0 }}>
                      <Text style={styles.serverName} numberOfLines={1}>
                        {server.name || server.hostname || "Máy chủ"}
                      </Text>
                      <Text style={styles.serverMeta} numberOfLines={1}>
                        {server.public_ip || server.hostname || `Docker ${server.docker_version ?? "—"}`}
                      </Text>
                    </View>
                  </View>
                  <Badge label={server.status} tone={statusTone(server.status)} />
                </View>

                {usage ? (
                  <View style={styles.metersRow}>
                    <MiniMeter label="CPU" value={usage.cpu_percent ?? 0} />
                    <MiniMeter label="RAM" value={usage.memory_percent ?? 0} />
                    <MiniMeter label="Đĩa" value={usage.disk_percent ?? 0} />
                  </View>
                ) : (
                  <Text style={styles.waitingText}>Đang chờ agent báo cáo…</Text>
                )}
              </Pressable>
            );
          })}
        </InsetGroup>
      )}

      {/* Recent Activity Timeline */}
      <SectionTitle>Hoạt động gần đây</SectionTitle>
      {events.isLoading ? (
        <Loading />
      ) : (events.data ?? []).length === 0 ? (
        <Empty text="Chưa có sự kiện nào." />
      ) : (
        <InsetGroup>
          {(events.data ?? []).slice(0, 6).map((event) => (
            <View key={event.id} style={styles.eventRow}>
              <View style={styles.eventHead}>
                <Text style={styles.eventTitle} numberOfLines={1}>
                  {event.title}
                </Text>
                <Text style={styles.eventTime}>{timeAgo(event.created_at)}</Text>
              </View>
              {event.message ? (
                <Text style={styles.eventMessage} numberOfLines={2}>
                  {event.message}
                </Text>
              ) : null}
            </View>
          ))}
        </InsetGroup>
      )}
    </Screen>
  );
}

const styles = StyleSheet.create({
  statusHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: spacing.sm,
    paddingHorizontal: 2,
  },
  statusLeft: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    flex: 1,
    minWidth: 0,
  },
  statusDot: {
    width: 7,
    height: 7,
    borderRadius: 3.5,
  },
  statusText: {
    color: colors.textSoft,
    fontSize: 13.5,
    fontWeight: "600",
  },
  versionTag: {
    color: colors.textFaint,
    fontFamily: monoFont,
    fontSize: 11.5,
  },
  quickRow: {
    flexDirection: "row",
    gap: spacing.sm,
  },
  quickChip: {
    flex: 1,
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    gap: 5,
    backgroundColor: colors.card,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: radius.md,
    paddingVertical: 10,
    paddingHorizontal: 4,
  },
  quickLabel: {
    color: colors.textSoft,
    fontSize: 11,
    fontWeight: "600",
  },
  serverRow: {
    paddingHorizontal: spacing.lg,
    paddingVertical: 14,
    gap: 12,
  },
  serverTop: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: spacing.sm,
  },
  serverIdentity: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    flex: 1,
    minWidth: 0,
  },
  serverDot: {
    width: 7,
    height: 7,
    borderRadius: 3.5,
  },
  serverName: {
    color: colors.text,
    fontSize: 14.5,
    fontWeight: "600",
    letterSpacing: -0.2,
  },
  serverMeta: {
    color: colors.textFaint,
    fontFamily: monoFont,
    fontSize: 11,
    marginTop: 2,
  },
  metersRow: {
    flexDirection: "row",
    gap: 12,
    paddingLeft: 17,
  },
  miniMeter: {
    flex: 1,
    gap: 5,
  },
  miniMeterHead: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "baseline",
  },
  miniMeterLabel: {
    color: colors.textDim,
    fontSize: 11,
    fontWeight: "500",
  },
  miniMeterVal: {
    color: colors.textSoft,
    fontFamily: monoFont,
    fontSize: 11,
    fontWeight: "600",
    fontVariant: ["tabular-nums"],
  },
  waitingText: {
    color: colors.textFaint,
    fontSize: 12,
    paddingLeft: 17,
  },
  eventRow: {
    paddingHorizontal: spacing.lg,
    paddingVertical: 11,
    gap: 3,
  },
  eventHead: {
    flexDirection: "row",
    alignItems: "baseline",
    justifyContent: "space-between",
    gap: spacing.sm,
  },
  eventTitle: {
    color: colors.text,
    fontSize: 13.5,
    fontWeight: "600",
    flex: 1,
  },
  eventTime: {
    color: colors.textFaint,
    fontFamily: monoFont,
    fontSize: 11,
  },
  eventMessage: {
    color: colors.textDim,
    fontSize: 12,
    lineHeight: 17,
  },
});

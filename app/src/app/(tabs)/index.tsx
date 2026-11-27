import { Ionicons } from "@expo/vector-icons";
import { useQuery } from "@tanstack/react-query";
import { router } from "expo-router";
import { StyleSheet, Text, View } from "react-native";
import { Badge, Card, Empty, ErrorBox, ListItem, Loading, Row, Screen, SectionTitle } from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { colors, spacing, statusTone } from "../../lib/theme";
import { timeAgo, type EventView, type MetricsOverview, type ServerView, type ServiceSummary, type SystemInfo } from "../../lib/types";

function MetricPill({ icon, label, value, tone }: { icon: keyof typeof Ionicons.glyphMap; label: string; value: string; tone: string }) {
  return (
    <View style={styles.metricPill}>
      <View style={[styles.metricIcon, { backgroundColor: tone + "22" }]}> 
        <Ionicons name={icon} color={tone} size={17} />
      </View>
      <Text style={styles.metricValue}>{value}</Text>
      <Text style={styles.metricLabel}>{label}</Text>
    </View>
  );
}

function UsageBar({ label, value, tone }: { label: string; value: number; tone: string }) {
  const percent = Math.max(0, Math.min(100, value));
  return (
    <View style={styles.usageBlock}>
      <View style={styles.usageHeader}>
        <Text style={styles.usageLabel}>{label}</Text>
        <Text style={styles.usageValue}>{Math.round(percent)}%</Text>
      </View>
      <View style={styles.usageTrack}>
        <View style={[styles.usageFill, { width: `${percent}%`, backgroundColor: tone }]} />
      </View>
    </View>
  );
}

export default function Dashboard() {
  const { api } = useAuth();
  const system = useQuery({ queryKey: ["system"], queryFn: () => api<SystemInfo>("/api/v1/system") });
  const services = useQuery({ queryKey: ["services"], queryFn: () => api<ServiceSummary[]>("/api/v1/services") });
  const servers = useQuery({ queryKey: ["servers"], queryFn: () => api<ServerView[]>("/api/v1/servers") });
  const metrics = useQuery({ queryKey: ["metrics-overview"], queryFn: () => api<MetricsOverview>("/api/v1/metrics/overview") });
  const events = useQuery({ queryKey: ["events"], queryFn: () => api<EventView[]>("/api/v1/events?limit=5") });

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
  const running = list.filter((s) => (s.status ?? "").toLowerCase().includes("running")).length;
  const trouble = list.filter((s) => {
    const v = (s.status ?? "").toLowerCase();
    return v.includes("error") || v.includes("fail") || v.includes("exited");
  });
  const online = (servers.data ?? []).filter((s) => (s.status ?? "").toLowerCase().includes("online")).length;

  return (
    <Screen refreshing={system.isRefetching} onRefresh={refetch}>
      <ErrorBox error={system.error ?? services.error} />

      <Card style={styles.heroCard}>
        <View style={styles.heroGlow} />
        <View style={styles.heroTitleRow}>
          <View style={styles.heroIcon}>
            <Ionicons name="compass" color={colors.accentBright} size={30} />
          </View>
          <View style={{ flex: 1 }}>
            <Text style={styles.heroKicker}>Rudder Control</Text>
            <Text style={styles.heroTitle}>Hạm đội của bạn</Text>
          </View>
          <Badge label={`v${system.data?.version ?? "—"}`} tone="busy" />
        </View>
        <View style={styles.metricsRow}>
          <MetricPill icon="cube" label="đang chạy" value={`${running}/${list.length}`} tone={colors.ok} />
          <MetricPill icon="server" label="máy chủ" value={`${online || servers.data?.length || 0}`} tone={colors.accentBright} />
          <MetricPill icon="alert-circle" label="cần xử lý" value={`${trouble.length}`} tone={trouble.length ? colors.danger : colors.ok} />
        </View>
      </Card>

      {servers.data?.map((server) => {
        const usage = metrics.data?.servers?.find((s) => s.server_id === server.id);
        return (
          <Card key={server.id}>
            <View style={styles.serverHeader}>
              <View style={{ flex: 1 }}>
                <Text style={styles.serverName}>{server.name || server.hostname || "Máy chủ"}</Text>
                <Text style={styles.serverMeta}>Docker {server.docker_version ?? "—"}</Text>
              </View>
              <Badge label={server.status} tone={statusTone(server.status)} />
            </View>
            {usage ? (
              <View style={{ gap: spacing.md, marginTop: spacing.sm }}>
                <UsageBar label="CPU" value={usage.cpu_percent ?? 0} tone={colors.accentBright} />
                <UsageBar label="Bộ nhớ" value={usage.memory_percent ?? 0} tone={colors.busy} />
                <UsageBar label="Đĩa" value={usage.disk_percent ?? 0} tone={(usage.disk_percent ?? 0) > 85 ? colors.danger : colors.ok} />
              </View>
            ) : (
              <Row label="Số liệu" value="Đang chờ agent báo cáo" dim />
            )}
          </Card>
        );
      })}

      {trouble.length > 0 ? (
        <>
          <SectionTitle>Cần chú ý</SectionTitle>
          {trouble.map((service) => (
            <ListItem
              key={service.id}
              title={service.name}
              subtitle={`${service.project_name} · ${service.environment_name}`}
              right={<Badge label={service.status} tone={statusTone(service.status)} />}
              onPress={() => router.push(`/service/${service.id}`)}
            />
          ))}
        </>
      ) : null}

      <SectionTitle>Sự kiện gần đây</SectionTitle>
      {events.isLoading ? (
        <Loading />
      ) : (events.data ?? []).length === 0 ? (
        <Empty text="Chưa có sự kiện nào." />
      ) : (
        (events.data ?? []).slice(0, 5).map((event) => (
          <Card key={event.id} style={styles.eventCard}>
            <View style={styles.eventHeader}>
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
          </Card>
        ))
      )}
    </Screen>
  );
}

const styles = StyleSheet.create({
  heroCard: { overflow: "hidden", backgroundColor: colors.bgElevated, gap: spacing.lg },
  heroGlow: {
    position: "absolute",
    top: -90,
    right: -70,
    width: 210,
    height: 210,
    borderRadius: 105,
    backgroundColor: colors.accentDim,
    opacity: 0.55,
  },
  heroTitleRow: { flexDirection: "row", alignItems: "center", gap: spacing.md },
  heroIcon: {
    width: 56,
    height: 56,
    borderRadius: 18,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.accentDim,
    borderWidth: 1,
    borderColor: colors.accent + "55",
  },
  heroKicker: { color: colors.accentBright, fontSize: 12, fontWeight: "700", letterSpacing: 1, textTransform: "uppercase" },
  heroTitle: { color: colors.text, fontSize: 28, fontWeight: "800", letterSpacing: -0.7, marginTop: 2 },
  metricsRow: { flexDirection: "row", gap: spacing.sm },
  metricPill: {
    flex: 1,
    borderRadius: 18,
    padding: spacing.md,
    backgroundColor: colors.cardSoft,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    gap: 3,
  },
  metricIcon: { width: 30, height: 30, borderRadius: 10, alignItems: "center", justifyContent: "center", marginBottom: 4 },
  metricValue: { color: colors.text, fontSize: 19, fontWeight: "800" },
  metricLabel: { color: colors.textDim, fontSize: 11, fontWeight: "600" },
  serverHeader: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: spacing.md },
  serverName: { color: colors.text, fontSize: 18, fontWeight: "800", letterSpacing: -0.3 },
  serverMeta: { color: colors.textDim, fontSize: 12, marginTop: 3 },
  usageBlock: { gap: 6 },
  usageHeader: { flexDirection: "row", justifyContent: "space-between" },
  usageLabel: { color: colors.textDim, fontSize: 12, fontWeight: "600" },
  usageValue: { color: colors.text, fontSize: 12, fontWeight: "700" },
  usageTrack: { height: 8, borderRadius: 4, overflow: "hidden", backgroundColor: colors.bg },
  usageFill: { height: 8, borderRadius: 4 },
  eventCard: { paddingVertical: spacing.md },
  eventHeader: { flexDirection: "row", justifyContent: "space-between", gap: spacing.sm },
  eventTitle: { color: colors.text, fontWeight: "700", flex: 1 },
  eventTime: { color: colors.textDim, fontSize: 11 },
  eventMessage: { color: colors.textDim, fontSize: 12, lineHeight: 17 },
});

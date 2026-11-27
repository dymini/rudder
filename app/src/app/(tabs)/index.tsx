import { useQuery } from "@tanstack/react-query";
import { router } from "expo-router";
import { Text, View } from "react-native";
import { Badge, Card, CardTitle, Empty, ErrorBox, ListItem, Loading, Row, Screen, SectionTitle } from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { statusTone } from "../../lib/theme";
import { timeAgo, type EventView, type MetricsOverview, type ServerView, type ServiceSummary, type SystemInfo } from "../../lib/types";

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

  return (
    <Screen refreshing={system.isRefetching} onRefresh={refetch}>
      <ErrorBox error={system.error ?? services.error} />

      <Card>
        <CardTitle>Panel</CardTitle>
        <Row label="Phiên bản" value={system.data?.version ?? "—"} />
        <Row label="Dịch vụ đang chạy" value={`${running}/${list.length}`} />
        <Row label="Máy chủ" value={String(servers.data?.length ?? "—")} />
      </Card>

      {servers.data?.map((server) => {
        const usage = metrics.data?.servers?.find((s) => s.server_id === server.id);
        return (
          <Card key={server.id}>
            <CardTitle>{server.name || server.hostname || "Máy chủ"}</CardTitle>
            <Row label="Trạng thái" value={<Badge label={server.status} tone={statusTone(server.status)} />} />
            {usage ? (
              <>
                <Row label="CPU" value={`${Math.round((usage.cpu_percent ?? 0) * 10) / 10}%`} />
                <Row label="RAM" value={`${Math.round(usage.memory_percent ?? 0)}%`} />
                <Row label="Đĩa" value={`${Math.round(usage.disk_percent ?? 0)}%`} />
              </>
            ) : null}
            <Row label="Docker" value={server.docker_version ?? "—"} dim />
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
          <Card key={event.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 8 }}>
              <Text style={{ color: "#e8edf5", fontWeight: "600", flex: 1 }} numberOfLines={1}>
                {event.title}
              </Text>
              <Text style={{ color: "#8b98ad", fontSize: 11 }}>{timeAgo(event.created_at)}</Text>
            </View>
            {event.message ? (
              <Text style={{ color: "#8b98ad", fontSize: 12 }} numberOfLines={2}>
                {event.message}
              </Text>
            ) : null}
          </Card>
        ))
      )}
    </Screen>
  );
}

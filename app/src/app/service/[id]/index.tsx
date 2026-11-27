import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { router, Stack, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { Alert, StyleSheet, Text, View } from "react-native";
import { Badge, Button, Card, CardTitle, ErrorBox, ListItem, Loading, Row, Screen, SectionTitle } from "../../../components/ui";
import { useAuth } from "../../../lib/auth";
import { colors, spacing, statusTone } from "../../../lib/theme";
import { formatTime, timeAgo, type DeploymentView, type ServiceUsage, type ServiceView, type MetricsOverview } from "../../../lib/types";

type Action = "deploy" | "start" | "stop" | "restart" | "rollback";

export default function ServiceDetail() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState<unknown>(null);

  const service = useQuery({
    queryKey: ["service", id],
    queryFn: () => api<ServiceView>(`/api/v1/services/${id}`),
  });
  const deployments = useQuery({
    queryKey: ["service-deployments", id],
    queryFn: () => api<DeploymentView[]>(`/api/v1/services/${id}/deployments`),
  });
  const metrics = useQuery({
    queryKey: ["metrics-overview"],
    queryFn: () => api<MetricsOverview>("/api/v1/metrics/overview"),
  });

  const mutation = useMutation({
    mutationFn: (action: Action) => api(`/api/v1/services/${id}/${action}`, { method: "POST", body: "{}" }),
    onSuccess: async () => {
      setActionError(null);
      await queryClient.invalidateQueries({ queryKey: ["service", id] });
      await queryClient.invalidateQueries({ queryKey: ["services"] });
    },
    onError: (error) => setActionError(error),
  });

  const remove = useMutation({
    mutationFn: () => api(`/api/v1/services/${id}`, { method: "DELETE" }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["services"] });
      router.replace("/(tabs)/services");
    },
    onError: (error) => setActionError(error),
  });

  function confirmDelete() {
    Alert.alert("Xóa dịch vụ", `Xóa "${data?.name}" vĩnh viễn? Container và volumes sẽ bị xóa.`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate() },
    ]);
  }

  function confirm(action: Action) {
    const labels: Record<Action, string> = {
      deploy: "Triển khai lại",
      start: "Khởi động",
      stop: "Dừng",
      restart: "Khởi động lại",
      rollback: "Rollback",
    };
    if (action === "deploy" || action === "restart" || action === "stop" || action === "rollback") {
      Alert.alert(labels[action], `Xác nhận ${labels[action].toLowerCase()} dịch vụ này?`, [
        { text: "Hủy", style: "cancel" },
        { text: labels[action], onPress: () => mutation.mutate(action) },
      ]);
    } else {
      mutation.mutate(action);
    }
  }

  if (service.isLoading) {
    return (
      <Screen scroll={false}>
        <Loading />
      </Screen>
    );
  }

  const data = service.data;
  const usage: ServiceUsage | undefined = metrics.data?.services?.find((s) => s.service_id === id);
  const lastDeployment = (deployments.data ?? [])[0];

  return (
    <Screen refreshing={service.isRefetching} onRefresh={() => service.refetch()}>
      <Stack.Screen options={{ title: data?.name ?? "Dịch vụ" }} />
      <ErrorBox error={service.error ?? actionError} />

      {data ? (
        <Card>
          <View style={styles.headerRow}>
            <Text style={styles.name}>{data.name}</Text>
            <Badge label={data.status} tone={statusTone(data.status)} />
          </View>
          <Row label="Loại" value={data.kind} dim />
          <Row label="Tạo lúc" value={formatTime(data.created_at)} dim />
          {usage ? (
            <>
              <Row label="CPU" value={`${Math.round((usage.cpu_percent ?? 0) * 10) / 10}%`} />
              <Row label="RAM" value={`${Math.round((usage.memory_bytes ?? 0) / 1024 / 1024)} MB`} />
            </>
          ) : null}
        </Card>
      ) : null}

      <Card>
        <CardTitle>Thao tác</CardTitle>
        <View style={styles.actions}>
          <Button title="Triển khai" small loading={mutation.isPending && mutation.variables === "deploy"} onPress={() => confirm("deploy")} />
          <Button title="Khởi động lại" small tone="ghost" loading={mutation.isPending && mutation.variables === "restart"} onPress={() => confirm("restart")} />
        </View>
        <View style={styles.actions}>
          <Button title="Khởi động" small tone="ghost" loading={mutation.isPending && mutation.variables === "start"} onPress={() => confirm("start")} />
          <Button title="Dừng" small tone="danger" loading={mutation.isPending && mutation.variables === "stop"} onPress={() => confirm("stop")} />
        </View>
        <View style={styles.actions}>
          <Button title="Rollback" small tone="ghost" loading={mutation.isPending && mutation.variables === "rollback"} onPress={() => confirm("rollback")} />
          <Button title="Xóa dịch vụ" small tone="danger" loading={remove.isPending} onPress={confirmDelete} />
        </View>
      </Card>

      <SectionTitle>Quản lý</SectionTitle>
      <ListItem title="Nhật ký (logs)" subtitle="Xem log container gần đây" onPress={() => router.push(`/service/${id}/logs`)} />
      <ListItem title="Biến môi trường" subtitle="Xem, thêm, sửa, xóa env" onPress={() => router.push(`/service/${id}/env`)} />
      <ListItem title="Tên miền" subtitle="Domain và SSL" onPress={() => router.push(`/service/${id}/domains`)} />
      <ListItem title="Compose stack" subtitle="Xem và sửa stack, thao tác compose" onPress={() => router.push(`/service/${id}/stack`)} />
      <ListItem title="Lịch sử triển khai" subtitle="Các lần deploy gần đây" onPress={() => router.push(`/service/${id}/deployments`)} />

      {lastDeployment ? (
        <>
          <SectionTitle>Triển khai gần nhất</SectionTitle>
          <Card>
            <Row label="Trạng thái" value={<Badge label={lastDeployment.status} tone={statusTone(lastDeployment.status)} />} />
            <Row label="Bắt đầu" value={timeAgo(lastDeployment.started_at ?? lastDeployment.created_at)} />
            {lastDeployment.error ? <Row label="Lỗi" value={lastDeployment.error} /> : null}
          </Card>
        </>
      ) : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  headerRow: { flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: spacing.sm },
  name: { color: colors.text, fontSize: 20, fontWeight: "700", flex: 1 },
  actions: { flexDirection: "row", gap: spacing.sm },
});

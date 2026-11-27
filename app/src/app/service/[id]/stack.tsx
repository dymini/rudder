import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { Alert, ScrollView, StyleSheet, Text, View } from "react-native";
import { Badge, Button, Card, CardTitle, ErrorBox, Loading, Row, Screen, SectionTitle } from "../../../components/ui";
import { useAuth } from "../../../lib/auth";
import { colors, spacing, statusTone } from "../../../lib/theme";
import type { StackView } from "../../../lib/types";

const ACTIONS = [
  { action: "up", label: "Up" },
  { action: "down", label: "Down" },
  { action: "pull", label: "Pull" },
  { action: "restart", label: "Restart" },
];

export default function ServiceStack() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);

  const stack = useQuery({
    queryKey: ["service-stack", id],
    queryFn: () => api<StackView>(`/api/v1/services/${id}/stack`),
    retry: false,
  });

  const act = useMutation({
    mutationFn: (action: string) => api(`/api/v1/services/${id}/stack/${action}`, { method: "POST", body: "{}" }),
    onSuccess: async () => {
      setError(null);
      await queryClient.invalidateQueries({ queryKey: ["service-stack", id] });
      await queryClient.invalidateQueries({ queryKey: ["service", id] });
    },
    onError: (err) => setError(err),
  });

  function confirm(action: string, label: string) {
    if (action === "down") {
      Alert.alert("Compose down", "Stack sẽ bị gỡ xuống. Xác nhận?", [
        { text: "Hủy", style: "cancel" },
        { text: label, style: "destructive", onPress: () => act.mutate(action) },
      ]);
    } else {
      act.mutate(action);
    }
  }

  if (stack.isLoading) {
    return (
      <Screen scroll={false}>
        <Loading />
      </Screen>
    );
  }

  if (stack.error) {
    return (
      <Screen>
        <Stack.Screen options={{ title: "Compose stack" }} />
        <ErrorBox error={stack.error} />
        <Text style={styles.note}>Dịch vụ này có thể không phải compose stack.</Text>
      </Screen>
    );
  }

  const data = stack.data;

  return (
    <Screen refreshing={stack.isRefetching} onRefresh={() => stack.refetch()}>
      <Stack.Screen options={{ title: "Compose stack" }} />
      <ErrorBox error={error} />
      {data ? (
        <>
          <Card>
            <CardTitle>Stack</CardTitle>
            <Row label="Nguồn" value={data.source} dim />
            {data.compose_path ? <Row label="Compose file" value={data.compose_path} dim /> : null}
            {data.main_service ? <Row label="Service chính" value={data.main_service} dim /> : null}
            <Row label="Cô lập mạng" value={data.network_isolation ? "Bật" : "Tắt"} dim />
          </Card>

          {(data.services ?? []).length > 0 ? (
            <>
              <SectionTitle>Services</SectionTitle>
              <Card>
                {(data.services ?? []).map((service) => (
                  <View key={service.name} style={styles.serviceRow}>
                    <Text style={styles.serviceName}>{service.name}</Text>
                    <Badge label={service.running ? "running" : "stopped"} tone={statusTone(service.running ? "running" : "stopped")} />
                  </View>
                ))}
              </Card>
            </>
          ) : null}

          <SectionTitle>Thao tác compose</SectionTitle>
          <View style={styles.actions}>
            {ACTIONS.map(({ action, label }) => (
              <Button
                key={action}
                title={label}
                small
                tone={action === "down" ? "danger" : "ghost"}
                loading={act.isPending && act.variables === action}
                onPress={() => confirm(action, label)}
              />
            ))}
          </View>

          {(data.warnings ?? []).length > 0 ? (
            <>
              <SectionTitle>Cảnh báo</SectionTitle>
              <Card>
                {(data.warnings ?? []).map((warning, index) => (
                  <Text key={index} style={styles.warning}>
                    • {warning}
                  </Text>
                ))}
              </Card>
            </>
          ) : null}

          <SectionTitle>Compose YAML</SectionTitle>
          <ScrollView style={styles.yamlBox} horizontal>
            <Text style={styles.yaml} selectable>
              {data.compose_yaml || data.generated_yaml || "(trống)"}
            </Text>
          </ScrollView>
        </>
      ) : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  note: { color: colors.textDim, fontSize: 13, padding: spacing.md },
  serviceRow: { flexDirection: "row", justifyContent: "space-between", alignItems: "center", paddingVertical: 4 },
  serviceName: { color: colors.text, fontSize: 14 },
  actions: { flexDirection: "row", flexWrap: "wrap", gap: spacing.sm },
  warning: { color: colors.warn, fontSize: 12 },
  yamlBox: {
    backgroundColor: "#060a10",
    borderRadius: 10,
    padding: spacing.md,
    maxHeight: 400,
  },
  yaml: { color: "#b8c4d8", fontFamily: "monospace" as never, fontSize: 11 },
});

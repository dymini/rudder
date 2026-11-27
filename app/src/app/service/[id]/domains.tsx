import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { Alert, StyleSheet, Text, TextInput, View } from "react-native";
import { Button, Card, Empty, ErrorBox, Loading, Row, Screen, SectionTitle } from "../../../components/ui";
import { useAuth } from "../../../lib/auth";
import { colors, spacing } from "../../../lib/theme";
import type { DomainView } from "../../../lib/types";

export default function ServiceDomains() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [host, setHost] = useState("");
  const [port, setPort] = useState("80");
  const [formError, setFormError] = useState<unknown>(null);

  const domains = useQuery({
    queryKey: ["service-domains", id],
    queryFn: () => api<DomainView[]>(`/api/v1/services/${id}/domains`),
  });

  async function refresh() {
    await queryClient.invalidateQueries({ queryKey: ["service-domains", id] });
  }

  const add = useMutation({
    mutationFn: () =>
      api(`/api/v1/services/${id}/domains`, {
        method: "POST",
        body: JSON.stringify({ host: host.trim(), target_port: Number(port) || 80, ssl_mode: "auto" }),
      }),
    onSuccess: async () => {
      setHost("");
      setFormError(null);
      await refresh();
    },
    onError: (error) => setFormError(error),
  });

  const remove = useMutation({
    mutationFn: (domainId: string) => api(`/api/v1/domains/${domainId}`, { method: "DELETE" }),
    onSuccess: refresh,
    onError: (error) => setFormError(error),
  });

  function confirmDelete(domain: DomainView) {
    Alert.alert("Xóa tên miền", `Xóa ${domain.host}?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(domain.id) },
    ]);
  }

  return (
    <Screen refreshing={domains.isRefetching} onRefresh={() => domains.refetch()}>
      <Stack.Screen options={{ title: "Tên miền" }} />
      <ErrorBox error={domains.error ?? formError} />

      {domains.isLoading ? (
        <Loading />
      ) : (domains.data ?? []).length === 0 ? (
        <Empty text="Chưa có tên miền nào." />
      ) : (
        (domains.data ?? []).map((domain) => (
          <Card key={domain.id}>
            <View style={styles.hostRow}>
              <Text style={styles.host}>{domain.host}</Text>
            </View>
            <Row label="Cổng đích" value={String(domain.target_port)} dim />
            <Row label="SSL" value={domain.ssl_mode} dim />
            <Row label="Xuất bản" value={domain.published ? "Có" : "Không"} dim />
            <View style={styles.actions}>
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(domain)} />
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Thêm tên miền</SectionTitle>
      <Card>
        <TextInput
          style={styles.input}
          value={host}
          onChangeText={setHost}
          placeholder="app.example.com"
          placeholderTextColor={colors.textDim}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
        />
        <TextInput
          style={styles.input}
          value={port}
          onChangeText={setPort}
          placeholder="Cổng container (80)"
          placeholderTextColor={colors.textDim}
          keyboardType="number-pad"
        />
        <Button title="Thêm tên miền" loading={add.isPending} disabled={!host.trim()} onPress={() => add.mutate()} />
      </Card>
    </Screen>
  );
}

const styles = StyleSheet.create({
  hostRow: { flexDirection: "row", alignItems: "center" },
  host: { color: colors.text, fontSize: 16, fontWeight: "600", flex: 1 },
  actions: { flexDirection: "row", gap: spacing.sm, marginTop: spacing.xs },
  input: {
    backgroundColor: colors.bg,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: 8,
    paddingHorizontal: spacing.md,
    paddingVertical: 10,
    color: colors.text,
    fontSize: 14,
  },
});

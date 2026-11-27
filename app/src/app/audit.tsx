import { useQuery } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { FlatList, StyleSheet, Text, View } from "react-native";
import { Button, Input, Loading, Screen, ErrorBox } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors, spacing } from "../lib/theme";
import { formatTime } from "../lib/types";

interface AuditView {
  id: number;
  action: string;
  actor_user_id?: string | null;
  created_at: string;
  ip?: string | null;
  resource_id?: string | null;
  resource_type: string;
}

export default function Audit() {
  const { api } = useAuth();
  const [filter, setFilter] = useState("");
  const [submitted, setSubmitted] = useState("");

  const audit = useQuery({
    queryKey: ["audit", submitted],
    queryFn: () => api<AuditView[]>(`/api/v1/audit?limit=200${submitted ? `&action=${encodeURIComponent(submitted)}` : ""}`),
  });

  return (
    <Screen scroll={false}>
      <Stack.Screen options={{ title: "Nhật ký audit" }} />
      <View style={styles.controls}>
        <View style={{ flex: 1 }}>
          <Input value={filter} onChangeText={setFilter} placeholder="Lọc theo action…" />
        </View>
        <Button title="Tìm" small onPress={() => setSubmitted(filter)} />
      </View>
      <ErrorBox error={audit.error} />
      {audit.isLoading ? (
        <Loading />
      ) : (
        <FlatList
          style={styles.list}
          contentContainerStyle={{ padding: spacing.md, gap: 6 }}
          data={audit.data ?? []}
          keyExtractor={(item) => String(item.id)}
          refreshing={audit.isRefetching}
          onRefresh={() => audit.refetch()}
          renderItem={({ item }) => (
            <View style={styles.row}>
              <Text style={styles.action}>
                {item.action} <Text style={styles.dim}>{item.resource_type}</Text>
              </Text>
              <Text style={styles.dim}>
                {formatTime(item.created_at)}
                {item.ip ? ` · ${item.ip}` : ""}
              </Text>
            </View>
          )}
          ListEmptyComponent={<Text style={styles.empty}>(trống)</Text>}
        />
      )}
    </Screen>
  );
}

const styles = StyleSheet.create({
  controls: { flexDirection: "row", gap: 8, padding: spacing.md, alignItems: "center" },
  list: { flex: 1, backgroundColor: colors.card, margin: spacing.md, marginTop: 0, borderRadius: 12 },
  row: { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: colors.cardBorder, paddingVertical: 5 },
  action: { color: colors.text, fontSize: 13, fontWeight: "600" },
  dim: { color: colors.textDim, fontSize: 11, fontWeight: "400" },
  empty: { color: colors.textDim, textAlign: "center", padding: spacing.lg },
});

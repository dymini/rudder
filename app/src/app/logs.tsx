import { useQuery } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { FlatList, StyleSheet, Text, View } from "react-native";
import { Button, ErrorBox, Input, Loading, Screen } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors, spacing } from "../lib/theme";

interface LogView {
  id: number;
  level: string;
  logged_at: string;
  message: string;
  source: string;
  http_status?: number | null;
}
interface LogPage {
  entries: LogView[];
  next_before_id?: number | null;
  next_before_time?: string | null;
}

const levelColor: Record<string, string> = {
  error: colors.danger,
  warn: colors.warn,
  info: colors.text,
  debug: colors.textDim,
};

export default function PanelLogs() {
  const { api } = useAuth();
  const [query, setQuery] = useState("");
  const [submitted, setSubmitted] = useState("");
  const [level, setLevel] = useState("");

  const logs = useQuery({
    queryKey: ["panel-logs", submitted, level],
    queryFn: () =>
      api<LogPage>(
        `/api/v1/logs?limit=200${submitted ? `&q=${encodeURIComponent(submitted)}` : ""}${level ? `&level=${level}` : ""}`,
      ),
    refetchInterval: 8000,
  });

  return (
    <Screen scroll={false}>
      <Stack.Screen options={{ title: "Logs panel" }} />
      <View style={styles.controls}>
        <View style={{ flex: 1 }}>
          <Input value={query} onChangeText={setQuery} placeholder="Lọc log…" />
        </View>
        <Button
          title={level || "mọi mức"}
          small
          tone="ghost"
          onPress={() => setLevel(level === "" ? "error" : level === "error" ? "warn" : level === "warn" ? "info" : "")}
        />
        <Button title="Tìm" small onPress={() => setSubmitted(query)} />
      </View>
      <ErrorBox error={logs.error} />
      {logs.isLoading ? (
        <Loading />
      ) : (
        <FlatList
          style={styles.list}
          contentContainerStyle={{ padding: spacing.md, gap: 4 }}
          data={logs.data?.entries ?? []}
          keyExtractor={(item) => String(item.id)}
          refreshing={logs.isRefetching}
          onRefresh={() => logs.refetch()}
          renderItem={({ item }) => (
            <View style={styles.row}>
              <Text style={[styles.meta, { color: levelColor[item.level] ?? colors.textDim }]}>
                {item.logged_at.slice(11, 19)} {item.level.slice(0, 4).toUpperCase()} {item.source}
                {item.http_status ? ` ${item.http_status}` : ""}
              </Text>
              <Text style={styles.message} numberOfLines={3}>
                {item.message}
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
  row: { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: colors.cardBorder, paddingVertical: 4 },
  meta: { fontSize: 10, fontWeight: "700" },
  message: { color: colors.text, fontSize: 12 },
  empty: { color: colors.textDim, textAlign: "center", padding: spacing.lg },
});

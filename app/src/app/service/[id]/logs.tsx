import { useQuery } from "@tanstack/react-query";
import { Stack, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import { ErrorBox, Loading, Screen } from "../../../components/ui";
import { useAuth } from "../../../lib/auth";
import { colors, spacing } from "../../../lib/theme";
import type { LogsView } from "../../../lib/types";

const TAILS = [200, 500, 1000];

export default function ServiceLogs() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { api } = useAuth();
  const [tail, setTail] = useState(200);

  const logs = useQuery({
    queryKey: ["service-logs", id, tail],
    queryFn: () => api<LogsView>(`/api/v1/services/${id}/logs?tail=${tail}`),
    refetchInterval: 5000,
  });

  return (
    <Screen scroll={false}>
      <Stack.Screen options={{ title: "Nhật ký" }} />
      <View style={styles.toolbar}>
        {TAILS.map((value) => (
          <Pressable key={value} onPress={() => setTail(value)} style={[styles.chip, tail === value && styles.chipActive]}>
            <Text style={[styles.chipText, tail === value && styles.chipTextActive]}>{value} dòng</Text>
          </Pressable>
        ))}
        <Pressable onPress={() => logs.refetch()} style={styles.chip}>
          <Text style={styles.chipText}>Tải lại</Text>
        </Pressable>
      </View>
      <ErrorBox error={logs.error} />
      {logs.isLoading ? (
        <Loading />
      ) : (
        <ScrollView style={styles.logScroll} contentContainerStyle={{ padding: spacing.md }}>
          <Text style={styles.logText} selectable>
            {logs.data?.logs?.trim() ? logs.data.logs : "(chưa có log)"}
          </Text>
        </ScrollView>
      )}
    </Screen>
  );
}

const styles = StyleSheet.create({
  toolbar: { flexDirection: "row", gap: spacing.sm, padding: spacing.md },
  chip: {
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: 999,
    paddingHorizontal: spacing.md,
    paddingVertical: 6,
  },
  chipActive: { borderColor: colors.accent, backgroundColor: colors.accentDim },
  chipText: { color: colors.textDim, fontSize: 12, fontWeight: "600" },
  chipTextActive: { color: colors.accent },
  logScroll: { flex: 1, backgroundColor: "#060a10", margin: spacing.md, marginTop: 0, borderRadius: 10 },
  logText: { color: "#b8c4d8", fontFamily: "monospace" as never, fontSize: 11, lineHeight: 15 },
});

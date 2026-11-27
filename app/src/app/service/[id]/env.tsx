import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { Alert, Pressable, StyleSheet, Text, TextInput, View } from "react-native";
import { Button, Card, CardTitle, Empty, ErrorBox, Loading, Screen, SectionTitle } from "../../../components/ui";
import { useAuth } from "../../../lib/auth";
import { colors, spacing } from "../../../lib/theme";
import type { EnvVarView } from "../../../lib/types";

export default function ServiceEnv() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [key, setKey] = useState("");
  const [value, setValue] = useState("");
  const [buildSecret, setBuildSecret] = useState(false);
  const [formError, setFormError] = useState<unknown>(null);

  const env = useQuery({
    queryKey: ["service-env", id],
    queryFn: () => api<EnvVarView[]>(`/api/v1/services/${id}/env`),
  });

  async function refresh() {
    await queryClient.invalidateQueries({ queryKey: ["service-env", id] });
  }

  const save = useMutation({
    mutationFn: () =>
      api(`/api/v1/services/${id}/env`, {
        method: "PUT",
        body: JSON.stringify({ key: key.trim(), value, is_build_secret: buildSecret }),
      }),
    onSuccess: async () => {
      setKey("");
      setValue("");
      setFormError(null);
      await refresh();
    },
    onError: (error) => setFormError(error),
  });

  const remove = useMutation({
    mutationFn: (name: string) => api(`/api/v1/services/${id}/env/${encodeURIComponent(name)}`, { method: "DELETE" }),
    onSuccess: refresh,
    onError: (error) => setFormError(error),
  });

  function confirmDelete(name: string) {
    Alert.alert("Xóa biến", `Xóa ${name}?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(name) },
    ]);
  }

  return (
    <Screen refreshing={env.isRefetching} onRefresh={() => env.refetch()}>
      <Stack.Screen options={{ title: "Biến môi trường" }} />
      <ErrorBox error={env.error ?? formError} />

      {env.isLoading ? (
        <Loading />
      ) : (env.data ?? []).length === 0 ? (
        <Empty text="Chưa có biến nào." />
      ) : (
        (env.data ?? []).map((item) => (
          <Pressable key={item.key} onLongPress={() => confirmDelete(item.key)} onPress={() => setKey(item.key)}>
            <Card>
              <View style={styles.envRow}>
                <Text style={styles.envKey}>{item.key}</Text>
                {item.is_build_secret ? <Text style={styles.envTag}>build secret</Text> : null}
              </View>
              <Text style={styles.envMeta}>
                giá trị {item.length} ký tự · chạm để sửa, giữ để xóa
              </Text>
            </Card>
          </Pressable>
        ))
      )}

      <SectionTitle>Thêm / cập nhật biến</SectionTitle>
      <Card>
        <TextInput
          style={styles.input}
          value={key}
          onChangeText={setKey}
          placeholder="TÊN_BIẾN"
          placeholderTextColor={colors.textDim}
          autoCapitalize="characters"
          autoCorrect={false}
        />
        <TextInput
          style={[styles.input, styles.valueInput]}
          value={value}
          onChangeText={setValue}
          placeholder="Giá trị"
          placeholderTextColor={colors.textDim}
          autoCapitalize="none"
          autoCorrect={false}
          multiline
        />
        <Pressable onPress={() => setBuildSecret((v) => !v)} style={styles.checkRow}>
          <View style={[styles.checkbox, buildSecret && styles.checkboxOn]} />
          <Text style={styles.checkLabel}>Build secret (chỉ dùng khi build image)</Text>
        </Pressable>
        <CardTitle> </CardTitle>
        <Button
          title="Lưu biến"
          loading={save.isPending}
          disabled={!key.trim()}
          onPress={() => save.mutate()}
        />
      </Card>
    </Screen>
  );
}

const styles = StyleSheet.create({
  envRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  envKey: { color: colors.text, fontSize: 14, fontWeight: "600", flex: 1 },
  envTag: { color: colors.warn, fontSize: 11 },
  envMeta: { color: colors.textDim, fontSize: 11 },
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
  valueInput: { minHeight: 60, textAlignVertical: "top" },
  checkRow: { flexDirection: "row", alignItems: "center", gap: spacing.sm },
  checkbox: {
    width: 18,
    height: 18,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: colors.cardBorder,
  },
  checkboxOn: { backgroundColor: colors.accent, borderColor: colors.accent },
  checkLabel: { color: colors.textDim, fontSize: 13 },
});

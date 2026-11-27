import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import * as WebBrowser from "expo-web-browser";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { timeAgo, type Schemas } from "../lib/types";

type GitProviderView = Schemas["GitProviderView"];

export default function GitProviders() {
  const { api, panelUrl } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);

  const providers = useQuery({ queryKey: ["git-providers"], queryFn: () => api<GitProviderView[]>("/api/v1/git-providers") });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["git-providers"] });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/git-providers/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const installLink = useMutation({
    mutationFn: async (id: string) => api<{ url?: string; install_url?: string }>(`/api/v1/git-providers/${id}/install-link`),
    onSuccess: async (data) => {
      const url = data.install_url ?? data.url;
      if (url) await WebBrowser.openBrowserAsync(url);
    },
    onError: setError,
  });

  function confirmDelete(provider: GitProviderView) {
    Alert.alert("Ngắt kết nối", `Gỡ "${provider.name}"? Service deploy từ git sẽ không cập nhật được nữa.`, [
      { text: "Hủy", style: "cancel" },
      { text: "Gỡ", style: "destructive", onPress: () => remove.mutate(provider.id) },
    ]);
  }

  return (
    <Screen refreshing={providers.isRefetching} onRefresh={() => providers.refetch()}>
      <Stack.Screen options={{ title: "Git providers" }} />
      <ErrorBox error={error ?? providers.error} />
      {providers.isLoading ? (
        <Loading />
      ) : (providers.data ?? []).length === 0 ? (
        <Empty text="Chưa kết nối Git provider nào." />
      ) : (
        (providers.data ?? []).map((provider) => (
          <Card key={provider.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{provider.name}</CardTitle>
              <Badge label={provider.status} tone={provider.status === "active" || provider.status === "ok" ? "ok" : "warn"} />
            </View>
            <Row label="Loại" value={provider.kind} dim />
            {provider.account ? <Row label="Tài khoản" value={provider.account} dim /> : null}
            <Row label="Kết nối" value={timeAgo(provider.created_at)} dim />
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
              {provider.kind === "github" ? (
                <Button title="Cài thêm repo" small tone="ghost" loading={installLink.isPending} onPress={() => installLink.mutate(provider.id)} />
              ) : null}
              <Button title="Gỡ" small tone="danger" onPress={() => confirmDelete(provider)} />
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Kết nối mới</SectionTitle>
      <Card>
        <CardTitle>Kết nối GitHub/GitLab</CardTitle>
        <Row
          label="Cách làm"
          value="Việc cài GitHub App cần trình duyệt có phiên đăng nhập — mở panel web để kết nối lần đầu, sau đó quản lý tại đây."
          dim
        />
        <Button title="Mở panel web" tone="ghost" onPress={() => panelUrl && WebBrowser.openBrowserAsync(`${panelUrl}/git`)} />
      </Card>
    </Screen>
  );
}

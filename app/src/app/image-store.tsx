import { useMutation, useQuery } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { bytes, timeAgo, type Schemas } from "../lib/types";

type ImageStoreOverview = Schemas["ImageStoreOverview"];

export default function ImageStore() {
  const { api } = useAuth();
  const [error, setError] = useState<unknown>(null);

  const overview = useQuery({ queryKey: ["image-store"], queryFn: () => api<ImageStoreOverview>("/api/v1/image-store/overview") });

  const test = useMutation({
    mutationFn: () => api<{ ok?: boolean; error?: string }>("/api/v1/image-store/test", { method: "POST", body: "{}" }),
    onError: setError,
  });

  const data = overview.data;

  return (
    <Screen refreshing={overview.isRefetching} onRefresh={() => overview.refetch()}>
      <Stack.Screen options={{ title: "Rudder Image" }} />
      <ErrorBox error={error ?? overview.error ?? (test.data?.error ? new Error(test.data.error) : null)} />
      {overview.isLoading ? (
        <Loading />
      ) : data ? (
        <>
          <Card>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>Tổng quan</CardTitle>
              <Badge
                label={data.destination ? data.destination.name : "chưa cấu hình"}
                tone={data.destination ? "ok" : "warn"}
              />
            </View>
            <Row label="Images" value={`${data.totals.images} (R2: ${data.totals.r2_images})`} />
            <Row label="Dung lượng R2" value={bytes(data.totals.r2_bytes)} />
            <Row label="Pushes 24h/30d" value={`${data.totals.pushes_24h} / ${data.totals.pushes_30d}`} dim />
            <Row label="Builds 30d" value={`${data.builds.succeeded_30d} ok / ${data.builds.failed_30d} lỗi`} dim />
            <Row label="Đang chạy/xếp" value={`${data.builds.running} / ${data.builds.queued}`} dim />
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
              <Button title="Test kết nối" small tone="ghost" loading={test.isPending} onPress={() => test.mutate()} />
            </View>
          </Card>

          <SectionTitle>Thiết lập</SectionTitle>
          {(data.setup ?? []).map((step) => (
            <Card key={step.key}>
              <Row label={step.key} value={<Badge label={step.state} tone={step.state === "done" || step.state === "ok" ? "ok" : step.state === "missing" ? "warn" : "dim"} />} />
              <Row label="" value={step.detail} dim />
            </Card>
          ))}

          <SectionTitle>Push gần đây</SectionTitle>
          {(data.recent_pushes ?? []).length === 0 ? (
            <Empty text="Chưa có push nào." />
          ) : (
            data.recent_pushes.slice(0, 15).map((push) => (
              <Card key={push.id}>
                <Row label="Image" value={`${push.path}:${push.tag}`} />
                <Row label="Dung lượng" value={bytes(push.size_bytes)} dim />
                <Row label="Lúc" value={timeAgo(push.created_at)} dim />
                {push.git_sha ? <Row label="Commit" value={push.git_sha.slice(0, 8)} dim /> : null}
              </Card>
            ))
          )}

          <SectionTitle>Clients</SectionTitle>
          {(data.clients ?? []).length === 0 ? (
            <Empty text="Chưa có client nào." />
          ) : (
            data.clients.map((client) => (
              <Card key={client.id}>
                <Row label="Tên" value={client.name} />
                <Row label="Scopes" value={client.scopes.join(", ")} dim />
                {client.last_used_at ? <Row label="Dùng cuối" value={timeAgo(client.last_used_at)} dim /> : null}
              </Card>
            ))
          )}
        </>
      ) : null}
    </Screen>
  );
}

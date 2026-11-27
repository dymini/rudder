import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import type { Schemas } from "../lib/types";

type DestinationView = Schemas["DestinationView"];

const blank = { name: "", endpoint: "", region: "us-east-1", bucket: "", access_key_id: "", secret_access_key: "" };

export default function S3Destinations() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState(blank);

  const destinations = useQuery({
    queryKey: ["s3-destinations"],
    queryFn: () => api<DestinationView[]>("/api/v1/s3-destinations"),
  });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["s3-destinations"] });
  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/s3-destinations", {
        method: "POST",
        body: JSON.stringify({
          name: form.name.trim(),
          endpoint: form.endpoint.trim() || null,
          region: form.region.trim() || "us-east-1",
          bucket: form.bucket.trim(),
          access_key_id: form.access_key_id || undefined,
          secret_access_key: form.secret_access_key || undefined,
          path_style: true,
        }),
      }),
    onSuccess: () => {
      setCreating(false);
      setForm(blank);
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/s3-destinations/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const test = useMutation({
    mutationFn: (id: string) => api<{ ok?: boolean; error?: string; message?: string }>(`/api/v1/s3-destinations/${id}/test`, { method: "POST", body: "{}" }),
    onSuccess: (result) => {
      if (result.error || result.ok === false) setError(new Error(result.error ?? result.message ?? "Test thất bại"));
      else Alert.alert("Test kết nối", "Kết nối thành công.");
    },
    onError: setError,
  });

  function confirmDelete(destination: DestinationView) {
    Alert.alert("Xóa destination", `Xóa "${destination.name}"? Backup jobs đang dùng sẽ lỗi.`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(destination.id) },
    ]);
  }

  return (
    <Screen refreshing={destinations.isRefetching} onRefresh={() => destinations.refetch()}>
      <Stack.Screen options={{ title: "S3 destinations" }} />
      <ErrorBox error={error ?? destinations.error} />
      {destinations.isLoading ? (
        <Loading />
      ) : (destinations.data ?? []).length === 0 ? (
        <Empty text="Chưa có destination nào." />
      ) : (
        (destinations.data ?? []).map((destination) => (
          <Card key={destination.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{destination.name}</CardTitle>
              <Badge label={destination.region} tone="dim" />
            </View>
            <Row label="Bucket" value={destination.bucket} dim />
            {destination.endpoint ? <Row label="Endpoint" value={destination.endpoint} dim /> : null}
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
              <Button title="Test" small tone="ghost" loading={test.isPending} onPress={() => test.mutate(destination.id)} />
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(destination)} />
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Thêm destination</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>S3 destination mới</CardTitle>
          <Input value={form.name} onChangeText={(v) => setForm({ ...form, name: v })} placeholder="Tên (vd Backups R2)" />
          <Input value={form.bucket} onChangeText={(v) => setForm({ ...form, bucket: v })} placeholder="Bucket" />
          <Input value={form.endpoint} onChangeText={(v) => setForm({ ...form, endpoint: v })} placeholder="Endpoint (bỏ trống nếu AWS)" keyboardType="url" />
          <Input value={form.region} onChangeText={(v) => setForm({ ...form, region: v })} placeholder="Region" />
          <Input value={form.access_key_id} onChangeText={(v) => setForm({ ...form, access_key_id: v })} placeholder="Access key ID" />
          <Input value={form.secret_access_key} onChangeText={(v) => setForm({ ...form, secret_access_key: v })} placeholder="Secret access key" secure />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!form.name.trim() || !form.bucket.trim()} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Destination mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

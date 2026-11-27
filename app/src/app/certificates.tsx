import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { formatTime, type Schemas } from "../lib/types";

type CertificateView = Schemas["CertificateView"];

export default function Certificates() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [certPem, setCertPem] = useState("");
  const [keyPem, setKeyPem] = useState("");
  const [chainPem, setChainPem] = useState("");

  const certs = useQuery({ queryKey: ["certificates"], queryFn: () => api<CertificateView[]>("/api/v1/certificates") });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["certificates"] });
  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/certificates", {
        method: "POST",
        body: JSON.stringify({
          name: name.trim(),
          cert_pem: certPem.trim(),
          key_pem: keyPem.trim(),
          chain_pem: chainPem.trim() || null,
        }),
      }),
    onSuccess: () => {
      setCreating(false);
      setName("");
      setCertPem("");
      setKeyPem("");
      setChainPem("");
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/certificates/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmDelete(cert: CertificateView) {
    Alert.alert("Xóa chứng chỉ", `Xóa "${cert.name}"?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(cert.id) },
    ]);
  }

  function expiryTone(iso?: string | null): "ok" | "warn" | "danger" | "dim" {
    if (!iso) return "dim";
    const days = (new Date(iso).getTime() - Date.now()) / 86400000;
    if (days < 0) return "danger";
    if (days < 14) return "warn";
    return "ok";
  }

  return (
    <Screen refreshing={certs.isRefetching} onRefresh={() => certs.refetch()}>
      <Stack.Screen options={{ title: "Chứng chỉ TLS" }} />
      <ErrorBox error={error ?? certs.error} />
      {certs.isLoading ? (
        <Loading />
      ) : (certs.data ?? []).length === 0 ? (
        <Empty text="Chưa có chứng chỉ nào." />
      ) : (
        (certs.data ?? []).map((cert) => (
          <Card key={cert.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{cert.name}</CardTitle>
              {cert.not_after ? (
                <Badge label={`hết hạn ${formatTime(cert.not_after)}`} tone={expiryTone(cert.not_after)} />
              ) : null}
            </View>
            {cert.common_name ? <Row label="CN" value={cert.common_name} dim /> : null}
            {cert.sans.length > 0 ? <Row label="SANs" value={cert.sans.join(", ")} dim /> : null}
            {cert.issuer ? <Row label="Issuer" value={cert.issuer} dim /> : null}
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
              <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(cert)} />
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Tải lên chứng chỉ</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Chứng chỉ mới</CardTitle>
          <Input value={name} onChangeText={setName} placeholder="Tên (vd wildcard example.com)" />
          <Input value={certPem} onChangeText={setCertPem} placeholder="cert PEM" multiline />
          <Input value={keyPem} onChangeText={setKeyPem} placeholder="private key PEM" multiline />
          <Input value={chainPem} onChangeText={setChainPem} placeholder="chain PEM (tuỳ chọn)" multiline />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button
              title="Tải lên"
              small
              loading={create.isPending}
              disabled={!name.trim() || !certPem.trim() || !keyPem.trim()}
              onPress={() => create.mutate()}
            />
          </View>
        </Card>
      ) : (
        <Button title="+ Chứng chỉ mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

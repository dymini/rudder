import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import type { UserView } from "../lib/types";

export default function Users() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [role, setRole] = useState("admin");

  const users = useQuery({ queryKey: ["users"], queryFn: () => api<UserView[]>("/api/v1/users") });

  const invalidate = () => queryClient.invalidateQueries({ queryKey: ["users"] });
  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/users", {
        method: "POST",
        body: JSON.stringify({
          email: email.trim(),
          password,
          display_name: displayName.trim() || null,
          role,
        }),
      }),
    onSuccess: () => {
      setCreating(false);
      setEmail("");
      setPassword("");
      setDisplayName("");
      invalidate();
    },
    onError: setError,
  });
  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/users/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });
  const patch = useMutation({
    mutationFn: ({ id, body }: { id: string; body: Record<string, unknown> }) =>
      api(`/api/v1/users/${id}`, { method: "PATCH", body: JSON.stringify(body) }),
    onSuccess: invalidate,
    onError: setError,
  });

  function confirmDelete(user: UserView) {
    Alert.alert("Xóa người dùng", `Xóa ${user.email}?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(user.id) },
    ]);
  }

  return (
    <Screen refreshing={users.isRefetching} onRefresh={() => users.refetch()}>
      <Stack.Screen options={{ title: "Người dùng" }} />
      <ErrorBox error={error ?? users.error} />
      {users.isLoading ? (
        <Loading />
      ) : (users.data ?? []).length === 0 ? (
        <Empty text="Không có người dùng nào." />
      ) : (
        (users.data ?? []).map((user) => (
          <Card key={user.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{user.display_name || user.email}</CardTitle>
              <Badge label={user.role} tone={user.is_owner ? "warn" : "dim"} />
            </View>
            <Row label="Email" value={user.email} dim />
            <Row label="TOTP" value={user.totp_enabled ? "đã bật" : "chưa bật"} dim />
            <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
              {!user.is_owner ? (
                <>
                  <Button
                    title={user.role === "admin" ? "Hạ viewer" : "Nâng admin"}
                    small
                    tone="ghost"
                    onPress={() => patch.mutate({ id: user.id, body: { role: user.role === "admin" ? "viewer" : "admin" } })}
                  />
                  <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(user)} />
                </>
              ) : null}
            </View>
          </Card>
        ))
      )}

      <SectionTitle>Thêm người dùng</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Người dùng mới</CardTitle>
          <Input value={email} onChangeText={setEmail} placeholder="Email" keyboardType="email-address" />
          <Input value={password} onChangeText={setPassword} placeholder="Mật khẩu" secure />
          <Input value={displayName} onChangeText={setDisplayName} placeholder="Tên hiển thị (tuỳ chọn)" />
          <Input value={role} onChangeText={setRole} placeholder="Quyền (admin/viewer)" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!email.trim() || !password} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Người dùng mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { timeAgo, type Schemas } from "../lib/types";

type AccessIdentity = Schemas["AccessIdentity"];
type AccessDepartment = Schemas["AccessDepartment"];
type AccessRank = Schemas["AccessRank"];

export default function AccessAdmin() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [addingDept, setAddingDept] = useState(false);
  const [deptName, setDeptName] = useState("");
  const [addingRank, setAddingRank] = useState(false);
  const [rankName, setRankName] = useState("");
  const [rankLevel, setRankLevel] = useState("10");
  const [inviting, setInviting] = useState(false);
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");

  const settings = useQuery({ queryKey: ["access-settings"], queryFn: () => api<Schemas["AccessSettingsView"]>("/api/v1/access/settings") });
  const departments = useQuery({ queryKey: ["access-departments"], queryFn: () => api<AccessDepartment[]>("/api/v1/access/departments") });
  const ranks = useQuery({ queryKey: ["access-ranks"], queryFn: () => api<AccessRank[]>("/api/v1/access/ranks") });
  const identities = useQuery({ queryKey: ["access-identities"], queryFn: () => api<AccessIdentity[]>("/api/v1/access/identities") });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ["access-departments"] });
    queryClient.invalidateQueries({ queryKey: ["access-ranks"] });
    queryClient.invalidateQueries({ queryKey: ["access-identities"] });
  };
  const createDept = useMutation({
    mutationFn: () => api("/api/v1/access/departments", { method: "POST", body: JSON.stringify({ name: deptName.trim() }) }),
    onSuccess: () => {
      setAddingDept(false);
      setDeptName("");
      invalidate();
    },
    onError: setError,
  });
  const createRank = useMutation({
    mutationFn: () => api("/api/v1/access/ranks", { method: "POST", body: JSON.stringify({ name: rankName.trim(), level: Number(rankLevel) }) }),
    onSuccess: () => {
      setAddingRank(false);
      setRankName("");
      invalidate();
    },
    onError: setError,
  });
  const invite = useMutation({
    mutationFn: () =>
      api("/api/v1/access/identities", { method: "POST", body: JSON.stringify({ email: email.trim(), display_name: displayName.trim() || null }) }),
    onSuccess: () => {
      setInviting(false);
      setEmail("");
      setDisplayName("");
      invalidate();
    },
    onError: setError,
  });
  const deleteIdentity = useMutation({
    mutationFn: (id: string) => api(`/api/v1/access/identities/${id}`, { method: "DELETE" }),
    onSuccess: invalidate,
    onError: setError,
  });

  return (
    <Screen
      refreshing={identities.isRefetching}
      onRefresh={() => {
        settings.refetch();
        departments.refetch();
        ranks.refetch();
        identities.refetch();
      }}
    >
      <Stack.Screen options={{ title: "Rudder Access" }} />
      <ErrorBox error={error ?? settings.error} />
      {settings.data ? (
        <Card>
          <CardTitle>Portal</CardTitle>
          <Row label="Host" value={settings.data.portal_host ?? "chưa cấu hình"} />
          <Row label="Email" value={settings.data.email_ready ? `sẵn sàng (${settings.data.email_provider})` : "chưa cấu hình"} dim />
          <Row label="Google login" value={settings.data.has_google_secret ? "đã cấu hình" : "chưa"} dim />
        </Card>
      ) : null}

      <SectionTitle>Định danh ({(identities.data ?? []).length})</SectionTitle>
      {(identities.data ?? []).map((person) => (
        <Card key={person.id}>
          <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
            <CardTitle>{person.display_name || person.email}</CardTitle>
            <Badge label={person.status} tone={person.status === "active" ? "ok" : "dim"} />
          </View>
          <Row label="Email" value={person.email} dim />
          <Row label="Vai trò" value={person.owner ? "owner" : person.admin ? "admin" : "user"} dim />
          {person.last_login_at ? <Row label="Login cuối" value={timeAgo(person.last_login_at)} dim /> : null}
          {!person.owner ? (
            <View style={{ flexDirection: "row", justifyContent: "flex-end" }}>
              <Button
                title="Xóa"
                small
                tone="danger"
                onPress={() =>
                  Alert.alert("Xóa định danh", `Xóa ${person.email}?`, [
                    { text: "Hủy", style: "cancel" },
                    { text: "Xóa", style: "destructive", onPress: () => deleteIdentity.mutate(person.id) },
                  ])
                }
              />
            </View>
          ) : null}
        </Card>
      ))}
      {inviting ? (
        <Card>
          <CardTitle>Mời người mới</CardTitle>
          <Input value={email} onChangeText={setEmail} placeholder="Email" keyboardType="email-address" />
          <Input value={displayName} onChangeText={setDisplayName} placeholder="Tên hiển thị" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setInviting(false)} />
            <Button title="Mời" small loading={invite.isPending} disabled={!email.trim()} onPress={() => invite.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Mời người dùng" tone="ghost" onPress={() => setInviting(true)} />
      )}

      <SectionTitle>Phòng ban ({(departments.data ?? []).length})</SectionTitle>
      {(departments.data ?? []).map((dept) => (
        <Card key={dept.id}>
          <Row label="Tên" value={dept.name} />
          {dept.description ? <Row label="Mô tả" value={dept.description} dim /> : null}
        </Card>
      ))}
      {(departments.data ?? []).length === 0 && !departments.isLoading ? <Empty text="Chưa có phòng ban." /> : null}
      {addingDept ? (
        <Card>
          <Input value={deptName} onChangeText={setDeptName} placeholder="Tên phòng ban" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setAddingDept(false)} />
            <Button title="Tạo" small loading={createDept.isPending} disabled={!deptName.trim()} onPress={() => createDept.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Phòng ban" small tone="ghost" onPress={() => setAddingDept(true)} />
      )}

      <SectionTitle>Cấp bậc ({(ranks.data ?? []).length})</SectionTitle>
      {(ranks.data ?? []).map((rank) => (
        <Card key={rank.id}>
          <Row label="Tên" value={rank.name} />
          <Row label="Level" value={rank.level} dim />
        </Card>
      ))}
      {(ranks.data ?? []).length === 0 && !ranks.isLoading ? <Empty text="Chưa có cấp bậc." /> : null}
      {addingRank ? (
        <Card>
          <Input value={rankName} onChangeText={setRankName} placeholder="Tên cấp bậc" />
          <Input value={rankLevel} onChangeText={setRankLevel} placeholder="Level (số)" keyboardType="numeric" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setAddingRank(false)} />
            <Button title="Tạo" small loading={createRank.isPending} disabled={!rankName.trim()} onPress={() => createRank.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Cấp bậc" small tone="ghost" onPress={() => setAddingRank(true)} />
      )}
    </Screen>
  );
}

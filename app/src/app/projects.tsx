import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { router, Stack } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Button, Card, CardTitle, Empty, ErrorBox, Input, ListItem, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { timeAgo, type EnvironmentView, type ProjectView } from "../lib/types";

export default function Projects() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  const projects = useQuery({ queryKey: ["projects"], queryFn: () => api<ProjectView[]>("/api/v1/projects") });
  const environments = useQuery({
    queryKey: ["all-environments"],
    queryFn: async () => {
      const result: Record<string, EnvironmentView[]> = {};
      for (const project of projects.data ?? []) {
        result[project.id] = await api<EnvironmentView[]>(`/api/v1/projects/${project.id}/environments`);
      }
      return result;
    },
    enabled: (projects.data ?? []).length > 0,
  });

  const create = useMutation({
    mutationFn: () =>
      api("/api/v1/projects", { method: "POST", body: JSON.stringify({ name: name.trim(), description: description.trim() || null }) }),
    onSuccess: () => {
      setCreating(false);
      setName("");
      setDescription("");
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: setError,
  });

  const remove = useMutation({
    mutationFn: (id: string) => api(`/api/v1/projects/${id}`, { method: "DELETE" }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["projects"] }),
    onError: setError,
  });

  function confirmDelete(project: ProjectView) {
    Alert.alert("Xóa project", `Xóa "${project.name}" và toàn bộ môi trường bên trong?`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => remove.mutate(project.id) },
    ]);
  }

  return (
    <Screen refreshing={projects.isRefetching} onRefresh={() => projects.refetch()}>
      <Stack.Screen options={{ title: "Projects" }} />
      <ErrorBox error={error ?? projects.error} />
      {projects.isLoading ? (
        <Loading />
      ) : (
        <>
          {(projects.data ?? []).map((project) => (
            <Card key={project.id}>
              <ListItem
                title={project.name}
                subtitle={`${(environments.data?.[project.id] ?? []).length} môi trường · tạo ${timeAgo(project.created_at)}`}
                onPress={() => router.push(`/project/${project.id}`)}
              />
              {project.description ? <Row label="Mô tả" value={project.description} dim /> : null}
              <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
                <Button title="Xóa" small tone="danger" onPress={() => confirmDelete(project)} />
              </View>
            </Card>
          ))}
          {(projects.data ?? []).length === 0 && <Empty text="Chưa có project nào." />}
        </>
      )}

      <SectionTitle>Tạo project</SectionTitle>
      {creating ? (
        <Card>
          <CardTitle>Project mới</CardTitle>
          <Input value={name} onChangeText={setName} placeholder="Tên project" />
          <Input value={description} onChangeText={setDescription} placeholder="Mô tả (tuỳ chọn)" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={create.isPending} disabled={!name.trim()} onPress={() => create.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Project mới" tone="ghost" onPress={() => setCreating(true)} />
      )}
    </Screen>
  );
}

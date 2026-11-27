import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { router, Stack, useLocalSearchParams } from "expo-router";
import { useState } from "react";
import { Alert, View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Input, ListItem, Loading, Row, Screen, SectionTitle } from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { statusTone } from "../../lib/theme";
import type { EnvironmentView, ProjectView, ServiceSummary } from "../../lib/types";

export default function ProjectDetail() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [creating, setCreating] = useState(false);
  const [envName, setEnvName] = useState("");

  const project = useQuery({ queryKey: ["project", id], queryFn: () => api<ProjectView>(`/api/v1/projects/${id}`) });
  const environments = useQuery({
    queryKey: ["environments", id],
    queryFn: () => api<EnvironmentView[]>(`/api/v1/projects/${id}/environments`),
  });
  const services = useQuery({ queryKey: ["services"], queryFn: () => api<ServiceSummary[]>("/api/v1/services") });

  const createEnv = useMutation({
    mutationFn: () =>
      api(`/api/v1/projects/${id}/environments`, { method: "POST", body: JSON.stringify({ name: envName.trim() }) }),
    onSuccess: () => {
      setCreating(false);
      setEnvName("");
      queryClient.invalidateQueries({ queryKey: ["environments", id] });
    },
    onError: setError,
  });

  const deleteEnv = useMutation({
    mutationFn: (envId: string) => api(`/api/v1/environments/${envId}`, { method: "DELETE" }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["environments", id] }),
    onError: setError,
  });

  function confirmDeleteEnv(env: EnvironmentView) {
    Alert.alert("Xóa môi trường", `Xóa "${env.name}"? Dịch vụ bên trong sẽ bị xóa.`, [
      { text: "Hủy", style: "cancel" },
      { text: "Xóa", style: "destructive", onPress: () => deleteEnv.mutate(env.id) },
    ]);
  }

  const envIds = new Set((environments.data ?? []).map((e) => e.id));
  const projectServices = (services.data ?? []).filter((s) => envIds.has(s.environment_id));

  return (
    <Screen
      refreshing={project.isRefetching}
      onRefresh={() => {
        project.refetch();
        environments.refetch();
        services.refetch();
      }}
    >
      <Stack.Screen options={{ title: project.data?.name ?? "Project" }} />
      <ErrorBox error={error ?? project.error} />
      {project.isLoading ? (
        <Loading />
      ) : project.data ? (
        <Card>
          <CardTitle>{project.data.name}</CardTitle>
          {project.data.description ? <Row label="Mô tả" value={project.data.description} dim /> : null}
        </Card>
      ) : null}

      <SectionTitle>Môi trường</SectionTitle>
      {(environments.data ?? []).map((env) => (
        <Card key={env.id}>
          <Row label="Tên" value={env.name} />
          <Row label="Dịch vụ" value={projectServices.filter((s) => s.environment_id === env.id).length} />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Xóa" small tone="danger" onPress={() => confirmDeleteEnv(env)} />
          </View>
        </Card>
      ))}
      {(environments.data ?? []).length === 0 && !environments.isLoading && <Empty text="Chưa có môi trường." />}

      {creating ? (
        <Card>
          <CardTitle>Môi trường mới</CardTitle>
          <Input value={envName} onChangeText={setEnvName} placeholder="Tên (production, staging…)" />
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setCreating(false)} />
            <Button title="Tạo" small loading={createEnv.isPending} disabled={!envName.trim()} onPress={() => createEnv.mutate()} />
          </View>
        </Card>
      ) : (
        <Button title="+ Môi trường mới" tone="ghost" onPress={() => setCreating(true)} />
      )}

      <SectionTitle>Dịch vụ</SectionTitle>
      {projectServices.length === 0 ? (
        <Empty text="Chưa có dịch vụ nào trong project này." />
      ) : (
        projectServices.map((service) => (
          <ListItem
            key={service.id}
            title={service.name}
            subtitle={`${service.environment_name} · ${service.kind}`}
            right={<Badge label={service.status} tone={statusTone(service.status)} />}
            onPress={() => router.push(`/service/${service.id}`)}
          />
        ))
      )}
    </Screen>
  );
}

import { useMutation, useQuery } from "@tanstack/react-query";
import { router, Stack } from "expo-router";
import { useState } from "react";
import { Text, View } from "react-native";
import { Button, Card, CardTitle, Empty, ErrorBox, Input, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors } from "../lib/theme";
import type { EnvironmentView, ProjectView, Schemas, ServerView } from "../lib/types";

type TemplateView = Schemas["TemplateView"];

export default function Templates() {
  const { api } = useAuth();
  const [error, setError] = useState<unknown>(null);
  const [selected, setSelected] = useState<TemplateView | null>(null);
  const [environmentId, setEnvironmentId] = useState("");
  const [serverId, setServerId] = useState("");
  const [name, setName] = useState("");
  const [values, setValues] = useState<Record<string, string>>({});
  const [deployed, setDeployed] = useState<Record<string, unknown> | null>(null);

  const templates = useQuery({ queryKey: ["templates"], queryFn: () => api<TemplateView[]>("/api/v1/templates") });
  const projects = useQuery({ queryKey: ["projects"], queryFn: () => api<ProjectView[]>("/api/v1/projects") });
  const servers = useQuery({ queryKey: ["servers"], queryFn: () => api<ServerView[]>("/api/v1/servers") });
  const environments = useQuery({
    queryKey: ["all-envs", (projects.data ?? []).length],
    queryFn: async () => {
      const all: (EnvironmentView & { project_name: string })[] = [];
      for (const project of projects.data ?? []) {
        const envs = await api<EnvironmentView[]>(`/api/v1/projects/${project.id}/environments`);
        for (const env of envs) all.push({ ...env, project_name: project.name });
      }
      return all;
    },
    enabled: (projects.data ?? []).length > 0,
  });

  const deploy = useMutation({
    mutationFn: () =>
      api<Record<string, unknown>>(`/api/v1/environments/${environmentId}/templates/${selected?.slug}/deploy`, {
        method: "POST",
        body: JSON.stringify({
          name: name.trim() || selected?.slug,
          server_id: serverId || servers.data?.[0]?.id,
          values: Object.fromEntries(Object.entries(values).filter(([, v]) => v !== "")),
        }),
      }),
    onSuccess: (result) => {
      setDeployed(result);
      setSelected(null);
    },
    onError: setError,
  });

  function openTemplate(template: TemplateView) {
    setSelected(template);
    setName(template.slug);
    setValues(Object.fromEntries(template.variables.map((v) => [v.name, v.default ?? ""])));
    setEnvironmentId(environments.data?.[0]?.id ?? "");
    setServerId(servers.data?.[0]?.id ?? "");
  }

  return (
    <Screen refreshing={templates.isRefetching} onRefresh={() => templates.refetch()}>
      <Stack.Screen options={{ title: "Templates" }} />
      <ErrorBox error={error ?? templates.error} />

      {deployed ? (
        <Card>
          <CardTitle>Đã triển khai</CardTitle>
          {typeof deployed.domain === "string" && deployed.domain ? <Row label="Tên miền" value={deployed.domain as string} /> : null}
          {typeof deployed.service_id === "string" ? (
            <Button title="Xem dịch vụ" small onPress={() => router.push(`/service/${deployed.service_id}`)} />
          ) : null}
          <Button title="Đóng" small tone="ghost" onPress={() => setDeployed(null)} />
        </Card>
      ) : null}

      {selected ? (
        <Card>
          <CardTitle>Deploy {selected.name}</CardTitle>
          <Input value={name} onChangeText={setName} placeholder="Tên dịch vụ" />
          <Text style={{ color: colors.textDim, fontSize: 12 }}>Môi trường:</Text>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
            {(environments.data ?? []).map((env) => (
              <Button
                key={env.id}
                title={`${env.project_name}/${env.name}`}
                small
                tone={environmentId === env.id ? "accent" : "ghost"}
                onPress={() => setEnvironmentId(env.id)}
              />
            ))}
          </View>
          <Text style={{ color: colors.textDim, fontSize: 12 }}>Server:</Text>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
            {(servers.data ?? []).map((server) => (
              <Button
                key={server.id}
                title={server.name}
                small
                tone={serverId === server.id ? "accent" : "ghost"}
                onPress={() => setServerId(server.id)}
              />
            ))}
          </View>
          {selected.variables.map((variable) => (
            <View key={variable.name}>
              <Text style={{ color: colors.textDim, fontSize: 12 }}>
                {variable.label || variable.name}
                {variable.required ? " *" : ""}
              </Text>
              <Input
                value={values[variable.name] ?? ""}
                onChangeText={(text) => setValues({ ...values, [variable.name]: text })}
                placeholder={variable.description}
              />
            </View>
          ))}
          <View style={{ flexDirection: "row", gap: 8, justifyContent: "flex-end" }}>
            <Button title="Hủy" small tone="ghost" onPress={() => setSelected(null)} />
            <Button title="Deploy" small loading={deploy.isPending} disabled={!environmentId} onPress={() => deploy.mutate()} />
          </View>
        </Card>
      ) : templates.isLoading ? (
        <Loading />
      ) : (templates.data ?? []).length === 0 ? (
        <Empty text="Không có template nào." />
      ) : (
        (templates.data ?? []).map((template) => (
          <Card key={template.slug}>
            <CardTitle>{template.name}</CardTitle>
            <Row label="Nhóm" value={template.category} dim />
            {template.description ? <Row label="" value={template.description} dim /> : null}
            <Button title="Deploy" small onPress={() => openTemplate(template)} />
          </Card>
        ))
      )}
      <SectionTitle> </SectionTitle>
    </Screen>
  );
}

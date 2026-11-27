import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { router, Stack } from "expo-router";
import { useState } from "react";
import { Text, View } from "react-native";
import { Button, Card, CardTitle, ErrorBox, Input, Loading, Screen } from "../components/ui";
import { useAuth } from "../lib/auth";
import { colors } from "../lib/theme";
import type { EnvironmentView, ProjectView, Schemas, ServerView } from "../lib/types";

const KINDS = [
  { value: "application", label: "Application" },
  { value: "image", label: "Docker image" },
  { value: "database", label: "Database" },
  { value: "compose", label: "Compose stack" },
];

export default function ServiceCreate() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [kind, setKind] = useState("application");
  const [serviceName, setServiceName] = useState("");
  const [environmentId, setEnvironmentId] = useState("");
  const [serverId, setServerId] = useState("");
  const [image, setImage] = useState("");
  const [engine, setEngine] = useState("postgres");
  const [version, setVersion] = useState("");
  const [stackYaml, setStackYaml] = useState("");
  const [mainService, setMainService] = useState("");

  const projects = useQuery({ queryKey: ["projects"], queryFn: () => api<ProjectView[]>("/api/v1/projects") });
  const servers = useQuery({ queryKey: ["servers"], queryFn: () => api<ServerView[]>("/api/v1/servers") });
  const engines = useQuery({
    queryKey: ["database-engines"],
    queryFn: () => api<Schemas["DatabaseEngineView"][]>("/api/v1/database-engines"),
    enabled: kind === "database",
  });
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

  const target = serverId || servers.data?.[0]?.id || "";
  const engineVersions = engines.data?.find((item) => item.engine === engine)?.versions ?? [];

  const create = useMutation({
    mutationFn: () =>
      kind === "database"
        ? api(`/api/v1/environments/${environmentId}/databases`, {
            method: "POST",
            body: JSON.stringify({
              name: serviceName.trim(),
              engine,
              version: version || engineVersions[0],
              server_id: target,
            }),
          })
        : kind === "compose"
          ? api(`/api/v1/environments/${environmentId}/stacks`, {
              method: "POST",
              body: JSON.stringify({
                name: serviceName.trim(),
                server_id: target,
                source: "inline",
                compose_yaml: stackYaml,
                main_service: mainService || undefined,
              }),
            })
          : api(`/api/v1/environments/${environmentId}/services`, {
              method: "POST",
              body: JSON.stringify({
                name: serviceName.trim(),
                kind,
                server_id: target,
                spec: image.trim() ? { image: image.trim() } : {},
              }),
            }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["services"] });
      router.back();
    },
    onError: setError,
  });

  if (projects.isLoading || servers.isLoading) {
    return (
      <Screen>
        <Stack.Screen options={{ title: "Tạo dịch vụ" }} />
        <Loading />
      </Screen>
    );
  }

  const ready =
    serviceName.trim() !== "" &&
    environmentId !== "" &&
    target !== "" &&
    (kind !== "compose" || stackYaml.trim() !== "");

  return (
    <Screen>
      <Stack.Screen options={{ title: "Tạo dịch vụ" }} />
      <ErrorBox error={error} />
      <Card>
        <CardTitle>Loại dịch vụ</CardTitle>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
          {KINDS.map((k) => (
            <Button key={k.value} title={k.label} small tone={kind === k.value ? "accent" : "ghost"} onPress={() => setKind(k.value)} />
          ))}
        </View>
      </Card>

      <Card>
        <Input value={serviceName} onChangeText={setServiceName} placeholder="Tên dịch vụ" />
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
              tone={target === server.id ? "accent" : "ghost"}
              onPress={() => setServerId(server.id)}
            />
          ))}
        </View>
      </Card>

      {kind === "image" || kind === "application" ? (
        <Card>
          <Input value={image} onChangeText={setImage} placeholder="Image (vd nginx:latest) — application để trống nếu build từ source" />
        </Card>
      ) : null}

      {kind === "database" ? (
        <Card>
          <Text style={{ color: colors.textDim, fontSize: 12 }}>Engine:</Text>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
            {(engines.data ?? []).map((item) => (
              <Button
                key={item.engine}
                title={item.engine}
                small
                tone={engine === item.engine ? "accent" : "ghost"}
                onPress={() => {
                  setEngine(item.engine);
                  setVersion("");
                }}
              />
            ))}
          </View>
          <Text style={{ color: colors.textDim, fontSize: 12 }}>Version:</Text>
          <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
            {engineVersions.map((v) => (
              <Button
                key={v}
                title={v}
                small
                tone={(version || engineVersions[0]) === v ? "accent" : "ghost"}
                onPress={() => setVersion(v)}
              />
            ))}
          </View>
        </Card>
      ) : null}

      {kind === "compose" ? (
        <Card>
          <Input value={mainService} onChangeText={setMainService} placeholder="Main service nhận domain (vd web)" />
          <Input value={stackYaml} onChangeText={setStackYaml} placeholder="Nội dung docker-compose.yml" multiline />
        </Card>
      ) : null}

      <Button title="Tạo dịch vụ" loading={create.isPending} disabled={!ready} onPress={() => create.mutate()} />
    </Screen>
  );
}

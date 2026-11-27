import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { type EnvironmentView, type ProjectView, type Schemas, type ServerView } from "../lib/types";

type PluginView = Schemas["PluginView"];

export default function Plugins() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [environmentId, setEnvironmentId] = useState("");
  const [serverId, setServerId] = useState("");

  const plugins = useQuery({ queryKey: ["plugins"], queryFn: () => api<PluginView[]>("/api/v1/plugins"), refetchInterval: 10000 });
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

  const check = useMutation({
    mutationFn: (slug: string) => api(`/api/v1/plugins/${slug}/check`, { method: "POST", body: "{}" }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["plugins"] }),
    onError: setError,
  });
  const install = useMutation({
    mutationFn: (args: { slug: string; plugin: PluginView }) =>
      api(`/api/v1/environments/${environmentId}/plugins/${args.slug}/install`, {
        method: "POST",
        body: JSON.stringify({
          name: args.plugin.manifest?.slug,
          server_id: serverId,
          version: args.plugin.last_check?.latest_version ?? "",
          values: Object.fromEntries((args.plugin.manifest?.variables ?? []).map((v) => [v.name, v.default ?? ""]).filter(([, v]) => v !== "")),
        }),
      }),
    onSuccess: () => {
      setInstalling(null);
      queryClient.invalidateQueries({ queryKey: ["plugins"] });
    },
    onError: setError,
  });

  return (
    <Screen refreshing={plugins.isRefetching} onRefresh={() => plugins.refetch()}>
      <Stack.Screen options={{ title: "Plugins" }} />
      <ErrorBox error={error ?? plugins.error} />
      {plugins.isLoading ? (
        <Loading />
      ) : (plugins.data ?? []).length === 0 ? (
        <Empty text="Không có plugin nào." />
      ) : (
        (plugins.data ?? []).map((plugin) => (
          <Card key={plugin.repo}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{plugin.manifest?.name ?? plugin.repo}</CardTitle>
              {(plugin.installs ?? []).length > 0 ? <Badge label={`${plugin.installs.length} đã cài`} tone="ok" /> : null}
            </View>
            {plugin.manifest?.description ? <Row label="" value={plugin.manifest.description} dim /> : null}
            {plugin.last_check?.latest_version ? <Row label="Bản mới nhất" value={plugin.last_check.latest_version} dim /> : null}
            {(plugin.installs ?? []).map((inst) => (
              <Row
                key={inst.id}
                label={inst.environment_name}
                value={`${inst.installed_version}${inst.latest_version && inst.latest_version !== inst.installed_version ? ` → ${inst.latest_version}` : ""}`}
                dim
              />
            ))}
            <View style={{ flexDirection: "row", gap: 8, flexWrap: "wrap", justifyContent: "flex-end" }}>
              <Button title="Kiểm tra update" small tone="ghost" loading={check.isPending} onPress={() => check.mutate(plugin.manifest?.slug ?? "")} />
              <Button
                title="Cài đặt"
                small
                onPress={() => {
                  setInstalling(installing === plugin.repo ? null : plugin.repo);
                  setEnvironmentId(environments.data?.[0]?.id ?? "");
                  setServerId(servers.data?.[0]?.id ?? "");
                }}
              />
            </View>
            {installing === plugin.repo ? (
              <>
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
                <Button
                  title="Xác nhận cài"
                  small
                  loading={install.isPending}
                  disabled={!environmentId}
                  onPress={() => install.mutate({ slug: plugin.manifest?.slug ?? "", plugin })}
                />
              </>
            ) : null}
          </Card>
        ))
      )}
      <SectionTitle> </SectionTitle>
    </Screen>
  );
}

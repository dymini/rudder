import { useQuery } from "@tanstack/react-query";
import { router } from "expo-router";
import { Fragment, useMemo, useState } from "react";
import { StyleSheet, TextInput } from "react-native";
import { Badge, Empty, ErrorBox, ListItem, Loading, Screen, SectionTitle } from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { colors, spacing, statusTone } from "../../lib/theme";
import type { ServiceSummary } from "../../lib/types";

export default function Services() {
  const { api } = useAuth();
  const [filter, setFilter] = useState("");
  const services = useQuery({ queryKey: ["services"], queryFn: () => api<ServiceSummary[]>("/api/v1/services") });

  const groups = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    const list = (services.data ?? []).filter((service) =>
      needle ? service.name.toLowerCase().includes(needle) : true,
    );
    const byProject = new Map<string, ServiceSummary[]>();
    for (const service of list) {
      const key = `${service.project_name} · ${service.environment_name}`;
      const bucket = byProject.get(key) ?? [];
      bucket.push(service);
      byProject.set(key, bucket);
    }
    return [...byProject.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [services.data, filter]);

  return (
    <Screen refreshing={services.isRefetching} onRefresh={() => services.refetch()}>
      <TextInput
        style={styles.search}
        value={filter}
        onChangeText={setFilter}
        placeholder="Tìm dịch vụ…"
        placeholderTextColor={colors.textDim}
        autoCapitalize="none"
      />
      <ErrorBox error={services.error} />
      {services.isLoading ? (
        <Loading />
      ) : groups.length === 0 ? (
        <Empty text="Chưa có dịch vụ nào." />
      ) : (
        groups.map(([group, list]) => (
          <Fragment key={group}>
            <SectionTitle>{group}</SectionTitle>
            {list.map((service) => (
              <ListItem
                key={service.id}
                title={service.name}
                subtitle={service.kind}
                right={<Badge label={service.status} tone={statusTone(service.status)} />}
                onPress={() => router.push(`/service/${service.id}`)}
              />
            ))}
          </Fragment>
        ))
      )}
    </Screen>
  );
}

const styles = StyleSheet.create({
  search: {
    backgroundColor: colors.card,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: 10,
    paddingHorizontal: spacing.md,
    paddingVertical: 10,
    color: colors.text,
    fontSize: 15,
  },
});

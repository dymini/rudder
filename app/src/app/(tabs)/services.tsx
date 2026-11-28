import { Ionicons } from "@expo/vector-icons";
import { useQuery } from "@tanstack/react-query";
import { router } from "expo-router";
import { Fragment, useMemo, useState } from "react";
import { Pressable, StyleSheet, TextInput, View } from "react-native";
import {
  Badge,
  Button,
  Empty,
  ErrorBox,
  GroupedRow,
  InsetGroup,
  Loading,
  Screen,
  SectionTitle,
  SegmentedControl,
} from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { colors, radius, spacing, statusTone } from "../../lib/theme";
import type { ServiceSummary } from "../../lib/types";

type StatusFilter = "all" | "running" | "issues" | "stopped";

const kindIcon = (kind: string | undefined): keyof typeof Ionicons.glyphMap => {
  if (kind === "database") return "server-outline";
  if (kind === "compose") return "layers-outline";
  return "cube-outline";
};

export default function Services() {
  const { api } = useAuth();
  const [filter, setFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const services = useQuery({
    queryKey: ["services"],
    queryFn: () => api<ServiceSummary[]>("/api/v1/services"),
  });

  const groups = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    const list = (services.data ?? []).filter((service) => {
      const st = (service.status ?? "").toLowerCase();
      if (statusFilter === "running" && !st.includes("running")) return false;
      if (
        statusFilter === "issues" &&
        !st.includes("error") &&
        !st.includes("fail") &&
        !st.includes("exited")
      ) {
        return false;
      }
      if (statusFilter === "stopped" && !st.includes("stopped")) return false;
      if (!needle) return true;
      return `${service.name} ${service.project_name} ${service.environment_name} ${service.kind}`
        .toLowerCase()
        .includes(needle);
    });
    const byProject = new Map<string, ServiceSummary[]>();
    for (const service of list) {
      const key = `${service.project_name} / ${service.environment_name}`;
      const bucket = byProject.get(key) ?? [];
      bucket.push(service);
      byProject.set(key, bucket);
    }
    return [...byProject.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [services.data, filter, statusFilter]);

  return (
    <Screen refreshing={services.isRefetching} onRefresh={() => services.refetch()}>
      <View style={styles.toolbar}>
        <View style={styles.searchShell}>
          <Ionicons name="search-outline" size={16} color={colors.textFaint} />
          <TextInput
            style={styles.search}
            value={filter}
            onChangeText={setFilter}
            placeholder="Tìm dịch vụ, dự án…"
            placeholderTextColor={colors.textFaint}
            autoCapitalize="none"
            autoCorrect={false}
          />
          {filter.length > 0 ? (
            <Pressable onPress={() => setFilter("")} hitSlop={8}>
              <Ionicons name="close-circle" size={16} color={colors.textFaint} />
            </Pressable>
          ) : null}
        </View>

        <SegmentedControl<StatusFilter>
          value={statusFilter}
          onChange={setStatusFilter}
          options={[
            { value: "all", label: "Tất cả" },
            { value: "running", label: "Chạy" },
            { value: "issues", label: "Sự cố" },
            { value: "stopped", label: "Dừng" },
          ]}
        />
      </View>

      <Button
        title="Tạo dịch vụ mới"
        tone="ghost"
        small
        onPress={() => router.push("/service-create")}
      />

      <ErrorBox error={services.error} />

      {services.isLoading ? (
        <Loading />
      ) : groups.length === 0 ? (
        <Empty text="Không tìm thấy dịch vụ phù hợp." />
      ) : (
        groups.map(([group, list]) => (
          <Fragment key={group}>
            <SectionTitle>{group}</SectionTitle>
            <InsetGroup>
              {list.map((service) => (
                <GroupedRow
                  key={service.id}
                  icon={kindIcon(service.kind)}
                  tone={statusTone(service.status)}
                  title={service.name}
                  subtitle={service.kind}
                  right={<Badge label={service.status} tone={statusTone(service.status)} />}
                  onPress={() => router.push(`/service/${service.id}`)}
                />
              ))}
            </InsetGroup>
          </Fragment>
        ))
      )}
    </Screen>
  );
}

const styles = StyleSheet.create({
  toolbar: {
    gap: spacing.sm,
  },
  searchShell: {
    flexDirection: "row",
    alignItems: "center",
    gap: spacing.sm,
    backgroundColor: colors.card,
    borderWidth: 1,
    borderColor: colors.cardBorder,
    borderRadius: radius.md,
    paddingHorizontal: spacing.md,
  },
  search: {
    flex: 1,
    paddingVertical: 10,
    color: colors.text,
    fontSize: 14.5,
  },
});

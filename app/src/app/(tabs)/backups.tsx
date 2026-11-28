import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment } from "react";
import { StyleSheet, Text, View } from "react-native";
import {
  Badge,
  Button,
  Card,
  Empty,
  ErrorBox,
  InsetGroup,
  Loading,
  Row,
  Screen,
  SectionTitle,
} from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { colors, monoFont, spacing, statusTone } from "../../lib/theme";
import { bytes, timeAgo, type BackupJobView, type BackupRunView } from "../../lib/types";

export default function Backups() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const jobs = useQuery({
    queryKey: ["backup-jobs"],
    queryFn: () => api<BackupJobView[]>("/api/v1/backup-jobs"),
  });

  const run = useMutation({
    mutationFn: (jobId: string) =>
      api(`/api/v1/backup-jobs/${jobId}/run`, { method: "POST", body: "{}" }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["backup-jobs"] }),
  });

  return (
    <Screen refreshing={jobs.isRefetching} onRefresh={() => jobs.refetch()}>
      <ErrorBox error={jobs.error ?? run.error} />
      {jobs.isLoading ? (
        <Loading />
      ) : (jobs.data ?? []).length === 0 ? (
        <Empty text="Chưa có tác vụ sao lưu nào. Tạo mới trên web panel." />
      ) : (
        (jobs.data ?? []).map((job) => (
          <Fragment key={job.id}>
            <Card>
              <View style={styles.jobHeader}>
                <View style={{ flex: 1, minWidth: 0 }}>
                  <Text style={styles.jobName} numberOfLines={1}>
                    {job.name}
                  </Text>
                  <Text style={styles.jobMeta}>{job.target_kind}</Text>
                </View>
                {job.last_run_at ? (
                  <Badge
                    label={job.last_status ?? "?"}
                    tone={statusTone(job.last_status)}
                  />
                ) : (
                  <Badge label="chưa chạy" tone="dim" />
                )}
              </View>
              {job.last_run_at ? (
                <Row label="Lần chạy cuối" value={timeAgo(job.last_run_at)} dim mono />
              ) : null}
              {job.next_run_at ? (
                <Row label="Lần chạy tiếp" value={timeAgo(job.next_run_at)} dim mono />
              ) : null}
              <View style={styles.jobFooter}>
                <Button
                  title="Chạy sao lưu ngay"
                  small
                  tone="ghost"
                  loading={run.isPending && run.variables === job.id}
                  disabled={!job.enabled}
                  onPress={() => run.mutate(job.id)}
                />
              </View>
            </Card>
            <JobRuns jobId={job.id} />
          </Fragment>
        ))
      )}
    </Screen>
  );
}

function JobRuns({ jobId }: { jobId: string }) {
  const { api } = useAuth();
  const runs = useQuery({
    queryKey: ["backup-runs", jobId],
    queryFn: () => api<BackupRunView[]>(`/api/v1/backup-jobs/${jobId}/runs`),
  });
  const list = (runs.data ?? []).slice(0, 3);
  if (list.length === 0) return null;
  return (
    <>
      <SectionTitle>Lịch sử chạy gần đây</SectionTitle>
      <InsetGroup>
        {list.map((item) => (
          <View key={item.id} style={styles.runRow}>
            <View style={styles.runHead}>
              <Badge label={item.status} tone={statusTone(item.status)} />
              <Text style={styles.runTime}>
                {item.size_bytes != null ? `${bytes(item.size_bytes)} · ` : ""}
                {timeAgo(item.started_at)}
              </Text>
            </View>
            {item.error ? (
              <Text style={styles.runError} numberOfLines={2}>
                {item.error}
              </Text>
            ) : null}
          </View>
        ))}
      </InsetGroup>
    </>
  );
}

const styles = StyleSheet.create({
  jobHeader: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: spacing.sm,
    marginBottom: 2,
  },
  jobName: {
    color: colors.text,
    fontSize: 15,
    fontWeight: "600",
    letterSpacing: -0.2,
  },
  jobMeta: {
    color: colors.textFaint,
    fontFamily: monoFont,
    fontSize: 11.5,
    marginTop: 2,
  },
  jobFooter: {
    marginTop: 4,
    alignItems: "flex-start",
  },
  runRow: {
    paddingHorizontal: spacing.lg,
    paddingVertical: 11,
    gap: 6,
  },
  runHead: {
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    gap: spacing.sm,
  },
  runTime: {
    color: colors.textDim,
    fontFamily: monoFont,
    fontSize: 11.5,
  },
  runError: {
    color: colors.danger,
    fontSize: 12,
    lineHeight: 17,
  },
});

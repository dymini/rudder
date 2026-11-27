import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment } from "react";
import { Badge, Button, Card, Empty, ErrorBox, Loading, Row, Screen, SectionTitle } from "../../components/ui";
import { useAuth } from "../../lib/auth";
import { statusTone } from "../../lib/theme";
import { bytes, timeAgo, type BackupJobView, type BackupRunView } from "../../lib/types";

export default function Backups() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const jobs = useQuery({ queryKey: ["backup-jobs"], queryFn: () => api<BackupJobView[]>("/api/v1/backup-jobs") });

  const run = useMutation({
    mutationFn: (jobId: string) => api(`/api/v1/backup-jobs/${jobId}/run`, { method: "POST", body: "{}" }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["backup-jobs"] }),
  });

  return (
    <Screen refreshing={jobs.isRefetching} onRefresh={() => jobs.refetch()}>
      <ErrorBox error={jobs.error ?? run.error} />
      {jobs.isLoading ? (
        <Loading />
      ) : (jobs.data ?? []).length === 0 ? (
        <Empty text="Chưa có backup job nào. Tạo job từ giao diện web panel." />
      ) : (
        (jobs.data ?? []).map((job) => (
          <Fragment key={job.id}>
            <Card>
              <Row label="" value="" />
              <Row label="Tên" value={job.name} />
              <Row label="Đích" value={job.target_kind} dim />
              <Row
                label="Lần chạy cuối"
                value={
                  job.last_run_at ? (
                    <Badge label={`${job.last_status ?? "?"} · ${timeAgo(job.last_run_at)}`} tone={statusTone(job.last_status)} />
                  ) : (
                    "chưa chạy"
                  )
                }
              />
              {job.next_run_at ? <Row label="Chạy tiếp" value={timeAgo(job.next_run_at)} dim /> : null}
              <Row label="" value="" />
              <Button
                title="Chạy ngay"
                small
                loading={run.isPending && run.variables === job.id}
                disabled={!job.enabled}
                onPress={() => run.mutate(job.id)}
              />
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
      <SectionTitle>Runs gần đây</SectionTitle>
      {list.map((item) => (
        <Card key={item.id}>
          <Row label="Trạng thái" value={<Badge label={item.status} tone={statusTone(item.status)} />} />
          <Row label="Bắt đầu" value={timeAgo(item.started_at)} dim />
          {item.size_bytes != null ? <Row label="Dung lượng" value={bytes(item.size_bytes)} dim /> : null}
          {item.error ? <Row label="Lỗi" value={item.error} /> : null}
        </Card>
      ))}
    </>
  );
}

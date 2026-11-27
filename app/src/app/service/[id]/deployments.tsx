import { useQuery } from "@tanstack/react-query";
import { Stack, useLocalSearchParams } from "expo-router";
import { Badge, Card, Empty, ErrorBox, Loading, Row, Screen } from "../../../components/ui";
import { useAuth } from "../../../lib/auth";
import { statusTone } from "../../../lib/theme";
import { timeAgo, type DeploymentView } from "../../../lib/types";

export default function ServiceDeployments() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { api } = useAuth();
  const deployments = useQuery({
    queryKey: ["service-deployments", id],
    queryFn: () => api<DeploymentView[]>(`/api/v1/services/${id}/deployments`),
  });

  return (
    <Screen refreshing={deployments.isRefetching} onRefresh={() => deployments.refetch()}>
      <Stack.Screen options={{ title: "Lịch sử triển khai" }} />
      <ErrorBox error={deployments.error} />
      {deployments.isLoading ? (
        <Loading />
      ) : (deployments.data ?? []).length === 0 ? (
        <Empty text="Chưa có lần triển khai nào." />
      ) : (
        (deployments.data ?? []).map((deployment) => (
          <Card key={deployment.id}>
            <Row label="Trạng thái" value={<Badge label={deployment.status} tone={statusTone(deployment.status)} />} />
            <Row label="Trigger" value={deployment.trigger} dim />
            <Row label="Bắt đầu" value={timeAgo(deployment.started_at ?? deployment.created_at)} dim />
            {deployment.image_ref ? <Row label="Image" value={deployment.image_ref} dim /> : null}
            {deployment.error ? <Row label="Lỗi" value={deployment.error} /> : null}
          </Card>
        ))
      )}
    </Screen>
  );
}

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { useState } from "react";
import { View } from "react-native";
import { Badge, Button, Card, CardTitle, Empty, ErrorBox, Loading, Row, Screen, SectionTitle } from "../components/ui";
import { useAuth } from "../lib/auth";
import { timeAgo, type Schemas } from "../lib/types";

export default function Security() {
  const { api } = useAuth();
  const queryClient = useQueryClient();
  const [error, setError] = useState<unknown>(null);

  const overview = useQuery({ queryKey: ["security-overview"], queryFn: () => api<Schemas["SecurityOverview"]>("/api/v1/security/overview") });
  const findings = useQuery({ queryKey: ["security-findings"], queryFn: () => api<Schemas["SecurityFindingView"][]>("/api/v1/security/findings") });
  const ssl = useQuery({ queryKey: ["security-ssl"], queryFn: () => api<Schemas["SslSummary"]>("/api/v1/security/ssl") });
  const scans = useQuery({ queryKey: ["security-scans"], queryFn: () => api<Schemas["SecurityScanView"][]>("/api/v1/security/scans") });

  const scan = useMutation({
    mutationFn: () => api("/api/v1/security/scans", { method: "POST", body: "{}" }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["security-scans"] });
      queryClient.invalidateQueries({ queryKey: ["security-findings"] });
    },
    onError: setError,
  });

  const severityTone = (severity: string) =>
    severity === "critical" || severity === "high" ? "danger" : severity === "medium" ? "warn" : "dim";

  return (
    <Screen
      refreshing={overview.isRefetching}
      onRefresh={() => {
        overview.refetch();
        findings.refetch();
        ssl.refetch();
        scans.refetch();
      }}
    >
      <Stack.Screen options={{ title: "Bảo mật" }} />
      <ErrorBox error={error ?? overview.error} />
      {overview.isLoading ? (
        <Loading />
      ) : overview.data ? (
        <Card>
          <CardTitle>Tổng quan</CardTitle>
          <Row label="Findings đang mở" value={overview.data.open_findings} />
          <Row label="Chứng chỉ" value={`${overview.data.certificates} (${overview.data.expiring} sắp hết hạn, ${overview.data.expired} đã hết)`} />
          <Row label="Requests HTTP / IP thô" value={`${overview.data.http_requests} / ${overview.data.ip_requests}`} dim />
          <Row label="Rate limit rules" value={overview.data.rate_rules} dim />
          <View style={{ flexDirection: "row", justifyContent: "flex-end" }}>
            <Button title="Quét ngay" small loading={scan.isPending} onPress={() => scan.mutate()} />
          </View>
        </Card>
      ) : null}

      <SectionTitle>Findings</SectionTitle>
      {(findings.data ?? []).length === 0 ? (
        <Empty text="Không có finding nào — tốt." />
      ) : (
        (findings.data ?? []).map((finding) => (
          <Card key={finding.id}>
            <View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}>
              <CardTitle>{finding.kind}</CardTitle>
              <Badge label={finding.severity} tone={severityTone(finding.severity)} />
            </View>
            <Row label="Host" value={finding.host} dim />
            {finding.path ? <Row label="Path" value={finding.path} dim /> : null}
            <Row label="" value={finding.summary} dim />
            <Row label="Phát hiện" value={timeAgo(finding.created_at)} dim />
          </Card>
        ))
      )}

      {ssl.data ? (
        <>
          <SectionTitle>SSL</SectionTitle>
          <Card>
            <Row label="Let's Encrypt" value={ssl.data.letsencrypt} />
            <Row label="Custom" value={ssl.data.custom} dim />
            <Row label="Không SSL" value={ssl.data.none} dim />
          </Card>
        </>
      ) : null}

      <SectionTitle>Lần quét</SectionTitle>
      {(scans.data ?? []).slice(0, 5).map((s) => (
        <Card key={s.id}>
          <Row label="Bắt đầu" value={timeAgo(s.started_at)} />
          <Row label="Trạng thái" value={<Badge label={s.status} tone={s.status === "done" ? "ok" : s.status === "failed" ? "danger" : "busy"} />} />
          {s.error ? <Row label="Lỗi" value={s.error} /> : null}
        </Card>
      ))}
    </Screen>
  );
}

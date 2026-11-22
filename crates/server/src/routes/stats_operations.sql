WITH deploys AS (
    SELECT count(*) AS n,
       count(*) FILTER (WHERE status='succeeded' AND finished_at IS NOT NULL)::float8/nullif(count(*) FILTER (WHERE finished_at IS NOT NULL),0) AS success,
       avg(extract(epoch FROM (finished_at-started_at))) FILTER (WHERE finished_at>=started_at) AS duration
    FROM deployments WHERE created_at>=$1 AND created_at<$2
), runs AS MATERIALIZED (
    SELECT job_id,status FROM backup_runs WHERE started_at>=$1 AND started_at<$2 AND kind='backup'
), run_counts AS (
    SELECT job_id,count(*) AS runs,count(*) FILTER (WHERE status IN ('failed','partial')) AS failures FROM runs GROUP BY job_id
), jobs AS (
    SELECT j.id AS job_id,j.name,coalesce(c.runs,0) AS runs,coalesce(c.failures,0) AS failures,
       (SELECT finished_at FROM backup_runs r WHERE r.job_id=j.id AND kind='backup' AND status='succeeded' AND finished_at<$2 ORDER BY finished_at DESC LIMIT 1) AS last_success
    FROM backup_jobs j LEFT JOIN run_counts c ON c.job_id=j.id
), mcp AS (
    SELECT count(*) AS n,coalesce(count(*) FILTER (WHERE status='error')::float8/nullif(count(*),0),0) AS rate,
       count(*) FILTER (WHERE status='denied') AS denied FROM mcp_calls
    WHERE created_at>=$1 AND created_at<$2 AND finished_at IS NOT NULL
)
SELECT jsonb_build_object(
    'deploys',d.n,'deploy_success_rate',d.success,'deploy_avg_duration_seconds',d.duration,
    'backup_runs',(SELECT count(*) FROM runs),'backup_failures',(SELECT count(*) FROM runs WHERE status IN ('failed','partial')),
    'backup_jobs',coalesce((SELECT jsonb_agg(to_jsonb(j) ORDER BY name,job_id) FROM jobs j),'[]'::jsonb),
    'alerts_fired',(SELECT count(*) FROM system_events WHERE kind='alert.fired' AND created_at>=$1 AND created_at<$2),
    'mcp_calls',m.n,'mcp_error_rate',m.rate,'mcp_denied',m.denied,
    'counts',jsonb_build_object('servers',(SELECT count(*) FROM servers),'services',(SELECT count(*) FROM services),'domains',(SELECT count(*) FROM domains),'plugins',(SELECT count(*) FROM plugin_installs))
) FROM deploys d CROSS JOIN mcp m

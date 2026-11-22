WITH marks AS (
    SELECT coalesce((SELECT done_until FROM metrics_watermarks WHERE resolution='1h'),
                    (SELECT max(bucket_start)+interval '1 hour' FROM metrics_rollups WHERE resolution='1h'),$1) AS h1,
           coalesce((SELECT done_until FROM metrics_watermarks WHERE resolution='5m'),
                    (SELECT max(bucket_start)+interval '5 minutes' FROM metrics_rollups WHERE resolution='5m'),$1) AS m5,
           coalesce((SELECT done_until FROM metrics_watermarks WHERE resolution='1m'),
                    (SELECT max(bucket_start)+interval '1 minute' FROM metrics_rollups WHERE resolution='1m'),$1) AS m1
), cuts AS (
    SELECT least($2,CASE WHEN $4='1h' THEN greatest($1,h1) ELSE $1 END) AS h1,
           least($2,CASE WHEN $4 IN ('1h','5m') THEN greatest($1,m5,CASE WHEN $4='1h' THEN h1 ELSE $1 END) ELSE $1 END) AS m5,
           least($2,greatest($1,m1,CASE WHEN $4 IN ('1h','5m') THEN m5 ELSE $1 END,CASE WHEN $4='1h' THEN h1 ELSE $1 END)) AS m1
    FROM marks
), tiers AS (
    SELECT '1h' AS resolution,3600.0 AS seconds,$1-make_interval(secs=>$3) AS lo,h1 AS hi FROM cuts WHERE $4='1h'
    UNION ALL SELECT '5m',300.0,CASE WHEN $4='1h' THEN h1 ELSE $1-make_interval(secs=>$3) END,m5 FROM cuts WHERE $4 IN ('1h','5m')
    UNION ALL SELECT '1m',60.0,CASE WHEN $4='1m' THEN $1-make_interval(secs=>$3) ELSE m5 END,m1 FROM cuts
), readings AS MATERIALIZED (
    SELECT r.bucket_start AS at,t.seconds,r.server_id,r.service_id,r.source,r.metric,
           r.avg_value AS value,r.max_value AS peak,r.sample_count::float8 AS n
    FROM tiers t JOIN metrics_rollups r ON r.resolution=t.resolution
      AND r.bucket_start>=t.lo AND r.bucket_start<t.hi
    WHERE r.source IN ('server','service','proxy')
      AND r.metric IN ('net_rx_bps','net_tx_bps','requests','http_5xx','cpu_percent','memory_percent','memory_bytes','restarts')
    UNION ALL
    SELECT r.sampled_at,10.0,r.server_id,r.service_id,r.source,r.metric,r.value,r.value,1.0
    FROM metrics_samples r,cuts c
    WHERE r.sampled_at>=CASE WHEN c.m1=$1 THEN $1-make_interval(secs=>$3) ELSE c.m1 END AND r.sampled_at<$2
      AND r.source IN ('server','service')
      AND r.metric IN ('net_rx_bps','net_tx_bps','cpu_percent','memory_percent','memory_bytes','restarts')
), weighted AS (
    SELECT *,date_bin(make_interval(secs=>$3),at,TIMESTAMPTZ 'epoch') AS bucket,
           greatest(0,extract(epoch FROM (least($2,at+make_interval(secs=>seconds))-greatest($1,at))))/seconds AS fraction
    FROM readings
    WHERE (source='server' AND service_id IS NULL) OR (source IN ('service','proxy') AND service_id IS NOT NULL)
), entities AS MATERIALIZED (
    SELECT bucket,server_id,service_id,
       sum(value*least(n*10,seconds)*fraction) FILTER (WHERE metric='net_rx_bps') AS rx,
       sum(value*least(n*10,seconds)*fraction) FILTER (WHERE metric='net_tx_bps') AS tx,
       sum(value*seconds/60*fraction) FILTER (WHERE metric='requests') AS requests,
       sum(value*seconds/60*fraction) FILTER (WHERE metric='http_5xx') AS errors,
       sum(value*n*fraction) FILTER (WHERE metric='cpu_percent')/nullif(sum(n*fraction) FILTER (WHERE metric='cpu_percent'),0) AS cpu,
       sum(n*fraction) FILTER (WHERE metric='cpu_percent') AS cpu_weight,
       max(peak) FILTER (WHERE metric='cpu_percent') AS cpu_peak,
       sum(value*n*fraction) FILTER (WHERE metric='memory_percent')/nullif(sum(n*fraction) FILTER (WHERE metric='memory_percent'),0) AS mem,
       max(peak) FILTER (WHERE metric='memory_percent') AS mem_peak,
       sum(value*n*fraction) FILTER (WHERE metric='memory_bytes')/nullif(sum(n*fraction) FILTER (WHERE metric='memory_bytes'),0) AS memory_bytes,
       sum(n*fraction) FILTER (WHERE metric='memory_bytes') AS memory_weight
    FROM weighted WHERE fraction>0
    GROUP BY GROUPING SETS ((bucket,server_id,service_id),(server_id,service_id))
), fleet AS (
    SELECT bucket AS at,sum(rx) FILTER (WHERE service_id IS NULL) AS rx_bytes,
       sum(tx) FILTER (WHERE service_id IS NULL) AS tx_bytes,
       coalesce(sum(requests),0) AS requests,coalesce(sum(errors),0) AS http_5xx,
       coalesce(sum(errors)/nullif(sum(requests),0),0) AS http_5xx_rate,
       avg(cpu) FILTER (WHERE service_id IS NULL) AS cpu_avg,max(cpu_peak) FILTER (WHERE service_id IS NULL) AS cpu_peak,
       avg(mem) FILTER (WHERE service_id IS NULL) AS memory_avg,max(mem_peak) FILTER (WHERE service_id IS NULL) AS memory_peak
    FROM entities GROUP BY bucket
), timeline AS (
    SELECT generate_series(date_bin(make_interval(secs=>$3),$1,TIMESTAMPTZ 'epoch'),date_bin(make_interval(secs=>$3),$2-interval '1 microsecond',TIMESTAMPTZ 'epoch'),make_interval(secs=>$3)) AS at
), service_totals AS MATERIALIZED (
    SELECT e.service_id,s.name,s.server_id,CASE WHEN count(rx)+count(tx)=0 THEN NULL ELSE sum(coalesce(rx,0)+coalesce(tx,0)) END AS network_bytes,
           sum(e.requests) AS requests,sum(e.errors) AS http_5xx,
           sum(e.cpu*e.cpu_weight)/nullif(sum(e.cpu_weight),0) AS cpu_percent,
           sum(e.memory_bytes*e.memory_weight)/nullif(sum(e.memory_weight),0) AS memory_bytes
    FROM entities e JOIN services s ON s.id=e.service_id WHERE bucket IS NULL
    GROUP BY e.service_id,s.name,s.server_id
), restart_changes AS (
    SELECT at,greatest(0,peak-lag(peak) OVER (PARTITION BY server_id,service_id ORDER BY at)) AS increase
    FROM readings WHERE metric='restarts' AND source='service'
), disk_readings AS (
    SELECT s.id AS server_id,m.metric,r.value,r.sampled_at AS at
    FROM servers s CROSS JOIN (VALUES ('disk_percent'),('disk_used_bytes')) m(metric)
    JOIN LATERAL (SELECT value,sampled_at FROM metrics_samples WHERE server_id=s.id AND metric=m.metric
      AND source='server' AND service_id IS NULL AND sampled_at >= $2-interval '30 days' AND sampled_at<$2
      ORDER BY sampled_at DESC,id DESC LIMIT 1) r ON TRUE
), disk_rollups AS (
    SELECT s.id AS server_id,m.metric,r.avg_value AS value,r.bucket_start AS at
    FROM servers s CROSS JOIN (VALUES ('disk_percent'),('disk_used_bytes')) m(metric)
    JOIN LATERAL (SELECT avg_value,bucket_start FROM metrics_rollups WHERE resolution='1h' AND server_id=s.id AND metric=m.metric
      AND source='server' AND service_id IS NULL AND bucket_start >= $2-interval '30 days' AND bucket_start<$2
      ORDER BY bucket_start DESC,id DESC LIMIT 1) r ON TRUE
), latest_disk AS (
    SELECT DISTINCT ON (server_id,metric) * FROM (SELECT * FROM disk_readings UNION ALL SELECT * FROM disk_rollups) d ORDER BY server_id,metric,at DESC
), disks AS (
    SELECT s.id AS server_id,s.name,max(d.value) FILTER (WHERE metric='disk_percent') AS disk_percent,
           max(d.value) FILTER (WHERE metric='disk_used_bytes') AS disk_used_bytes,max(d.at) AS sampled_at
    FROM servers s LEFT JOIN latest_disk d ON d.server_id=s.id GROUP BY s.id,s.name
)
SELECT jsonb_build_object(
    'totals',coalesce((SELECT to_jsonb(f)-'at' FROM fleet f WHERE at IS NULL),jsonb_build_object('rx_bytes',NULL,'tx_bytes',NULL,'requests',0,'http_5xx',0,'http_5xx_rate',0,'cpu_avg',NULL,'cpu_peak',NULL,'memory_avg',NULL,'memory_peak',NULL)),
    'series',(SELECT jsonb_agg(jsonb_build_object('at',t.at,'rx_bytes',f.rx_bytes,'tx_bytes',f.tx_bytes,'requests',coalesce(f.requests,0),'http_5xx',coalesce(f.http_5xx,0),'http_5xx_rate',coalesce(f.http_5xx_rate,0),'cpu_avg',f.cpu_avg,'cpu_peak',f.cpu_peak,'memory_avg',f.memory_avg,'memory_peak',f.memory_peak) ORDER BY t.at) FROM timeline t LEFT JOIN fleet f ON f.at=t.at),
    'top_network',coalesce((SELECT jsonb_agg(to_jsonb(s)) FROM (SELECT * FROM service_totals WHERE network_bytes IS NOT NULL ORDER BY network_bytes DESC,service_id LIMIT 10) s),'[]'::jsonb),
    'top_requests',coalesce((SELECT jsonb_agg(to_jsonb(s)) FROM (SELECT * FROM service_totals WHERE requests IS NOT NULL ORDER BY requests DESC,service_id LIMIT 10) s),'[]'::jsonb),
    'top_5xx',coalesce((SELECT jsonb_agg(to_jsonb(s)) FROM (SELECT * FROM service_totals WHERE http_5xx IS NOT NULL ORDER BY http_5xx DESC,service_id LIMIT 10) s),'[]'::jsonb),
    'top_cpu',coalesce((SELECT jsonb_agg(to_jsonb(s)) FROM (SELECT * FROM service_totals WHERE cpu_percent IS NOT NULL ORDER BY cpu_percent DESC,service_id LIMIT 10) s),'[]'::jsonb),
    'top_memory',coalesce((SELECT jsonb_agg(to_jsonb(s)) FROM (SELECT * FROM service_totals WHERE memory_bytes IS NOT NULL ORDER BY memory_bytes DESC,service_id LIMIT 10) s),'[]'::jsonb),
    'container_restarts',coalesce((SELECT sum(increase) FROM restart_changes WHERE at>=$1 AND at<$2),0),
    'disks',coalesce((SELECT jsonb_agg(to_jsonb(d) ORDER BY name,server_id) FROM disks d),'[]'::jsonb)
)

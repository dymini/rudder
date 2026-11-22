use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{DateTime, Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::HashMap;
use tower::ServiceExt;
use uuid::Uuid;

async fn fixture(pool: &PgPool) -> (axum::Router, String, Uuid) {
    let vars = HashMap::from([
        (
            "DATABASE_URL".into(),
            std::env::var("DATABASE_URL").unwrap(),
        ),
        ("RUDDER_MASTER_KEY".into(), "cc".repeat(32)),
        ("RUDDER_PUBLIC_URL".into(), "https://panel.example".into()),
    ]);
    let app = rudder_server::router(rudder_server::AppState::new(
        pool.clone(),
        rudder_server::Config::from_map(&vars, |_| unreachable!()).unwrap(),
    ));
    let user:Uuid=sqlx::query_scalar("INSERT INTO users(email,password_hash,role) VALUES ('stats@example.com','unused','admin') RETURNING id").fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO oauth_clients(client_id,client_name,redirect_uris) VALUES ('stats','Stats',ARRAY['http://localhost/callback'])").execute(pool).await.unwrap();
    let grant:Uuid=sqlx::query_scalar("INSERT INTO mcp_grants(user_id,client_id,name,template,scopes) VALUES ($1,'stats','Stats','custom',ARRAY['monitoring:read']) RETURNING id").bind(user).fetch_one(pool).await.unwrap();
    let token = format!("rdr_mca_{}", Uuid::new_v4());
    sqlx::query("INSERT INTO mcp_access_tokens(token_hash,grant_id,expires_at) VALUES ($1,$2,now()+interval '1 hour')").bind(hex::encode(Sha256::digest(token.as_bytes()))).bind(grant).execute(pool).await.unwrap();
    (app, token, grant)
}
async fn get(app: &axum::Router, token: &str, range: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/stats/overview?range={range}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn rollup(
    pool: &PgPool,
    at: DateTime<Utc>,
    resolution: &str,
    server: Uuid,
    service: Option<Uuid>,
    metric: &str,
    value: f64,
) {
    let source = if ["requests", "http_5xx"].contains(&metric) {
        "proxy"
    } else if service.is_some() {
        "service"
    } else {
        "server"
    };
    let n = match resolution {
        "1h" => 360,
        "5m" => 30,
        _ => 6,
    };
    sqlx::query("INSERT INTO metrics_rollups(bucket_start,resolution,server_id,service_id,source,metric,avg_value,min_value,max_value,sample_count) VALUES ($1,$2,$3,$4,$5,$6,$7,$7,$7,$8)").bind(at).bind(resolution).bind(server).bind(service).bind(source).bind(metric).bind(value).bind(n).execute(pool).await.unwrap();
}
async fn raw(
    pool: &PgPool,
    at: DateTime<Utc>,
    server: Uuid,
    service: Option<Uuid>,
    metric: &str,
    value: f64,
) {
    sqlx::query("INSERT INTO metrics_samples(sampled_at,server_id,service_id,source,metric,value) VALUES ($1,$2,$3,$4,$5,$6)").bind(at).bind(server).bind(service).bind(if service.is_some(){"service"}else{"server"}).bind(metric).bind(value).execute(pool).await.unwrap();
}
async fn server(pool: &PgPool, name: &str) -> Uuid {
    sqlx::query_scalar("INSERT INTO servers(name,kind) VALUES ($1,'local') RETURNING id")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn empty_ranges_validation_and_monitoring_scope(pool: PgPool) {
    let (app, token, grant) = fixture(&pool).await;
    for range in ["1h", "24h", "7d", "30d"] {
        let (status, body) = get(&app, &token, range).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["operations"]["counts"]["services"], 0);
        assert_eq!(body["metrics"]["totals"]["requests"].as_f64(), Some(0.0));
        assert!(body["metrics"]["totals"]["cpu_avg"].is_null());
        assert!(body["operations"]["deploy_success_rate"].is_null());
        assert!(body["metrics"]["series"].as_array().unwrap().len() <= 722);
        assert_eq!(body["metrics"]["top_cpu"], json!([]));
    }
    assert_eq!(get(&app, &token, "bad").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(get(&app, "invalid", "1h").await.0, StatusCode::UNAUTHORIZED);
    let response=app.clone().oneshot(Request::builder().method("POST").uri("/mcp").header("authorization",format!("Bearer {token}")).header("content-type","application/json").body(Body::from(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_v1_stats_overview","arguments":{"query":{"range":"1h"}}}}).to_string())).unwrap()).await.unwrap();
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["result"]["isError"], false, "{body}");
    sqlx::query("UPDATE mcp_grants SET scopes=ARRAY['auth:read'] WHERE id=$1")
        .bind(grant)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(get(&app, &token, "1h").await.0, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "./migrations")]
async fn fleet_traffic_tails_rankings_operations_and_disk(pool: PgPool) {
    let (app, token, _) = fixture(&pool).await;
    let a = server(&pool, "a").await;
    let b = server(&pool, "b").await;
    let project: Uuid =
        sqlx::query_scalar("INSERT INTO projects(name) VALUES ('stats') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let env: Uuid = sqlx::query_scalar(
        "INSERT INTO environments(project_id,name) VALUES ($1,'prod') RETURNING id",
    )
    .bind(project)
    .fetch_one(&pool)
    .await
    .unwrap();
    let at: DateTime<Utc> =
        sqlx::query_scalar("SELECT date_trunc('hour',now())-interval '2 hours'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut services = Vec::new();
    for i in 0..12 {
        let id:Uuid=sqlx::query_scalar("INSERT INTO services(environment_id,server_id,name,kind) VALUES ($1,$2,$3,'application') RETURNING id").bind(env).bind(a).bind(format!("svc-{i}")).fetch_one(&pool).await.unwrap();
        rollup(
            &pool,
            at,
            "5m",
            a,
            Some(id),
            "cpu_percent",
            if i == 0 { 99.0 } else { f64::from(i) },
        )
        .await;
        services.push(id);
    }
    let service = services[0];
    for (res, t) in [
        ("5m", at + Duration::minutes(5)),
        ("1m", at + Duration::minutes(6)),
    ] {
        sqlx::query("INSERT INTO metrics_watermarks(resolution,done_until) VALUES ($1,$2)")
            .bind(res)
            .bind(t)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (id, rx, cpu, mem) in [(a, 100.0, 20.0, 30.0), (b, 200.0, 60.0, 70.0)] {
        for (metric, value) in [
            ("net_rx_bps", rx),
            ("net_tx_bps", if id == a { 10.0 } else { 50.0 }),
            ("cpu_percent", cpu),
            ("memory_percent", mem),
        ] {
            rollup(&pool, at, "5m", id, None, metric, value).await;
        }
    }
    rollup(&pool, at, "5m", a, Some(service), "net_rx_bps", 999999.0).await;
    rollup(&pool, at, "5m", a, Some(service), "memory_bytes", 1024.0).await;
    rollup(&pool, at, "5m", a, Some(service), "requests", 10.0).await;
    rollup(&pool, at, "5m", a, Some(service), "http_5xx", 1.0).await;
    rollup(
        &pool,
        at + Duration::minutes(5),
        "1m",
        a,
        Some(service),
        "requests",
        2.0,
    )
    .await;
    rollup(
        &pool,
        at + Duration::minutes(5),
        "1m",
        a,
        Some(service),
        "http_5xx",
        1.0,
    )
    .await;
    rollup(
        &pool,
        at + Duration::minutes(2),
        "1m",
        a,
        None,
        "net_rx_bps",
        1000000.0,
    )
    .await;
    raw(&pool, at, a, None, "net_rx_bps", 1000000.0).await;
    let tail = at + Duration::minutes(6) + Duration::seconds(10);
    raw(&pool, tail, a, None, "net_rx_bps", 50.0).await;
    raw(&pool, tail, a, None, "net_tx_bps", 5.0).await;
    raw(&pool, tail, a, None, "cpu_percent", 20.0).await;
    rollup(&pool, at, "5m", a, Some(service), "restarts", 10.0).await;
    rollup(
        &pool,
        at + Duration::minutes(5),
        "1m",
        a,
        Some(service),
        "restarts",
        12.0,
    )
    .await;
    raw(&pool, tail, a, Some(service), "restarts", 13.0).await;
    raw(&pool, tail, a, None, "disk_percent", 70.0).await;
    rollup(&pool, at, "1h", b, None, "disk_percent", 50.0).await;
    sqlx::query("INSERT INTO deployments(service_id,status,started_at,finished_at,created_at) VALUES ($1,'succeeded',$2,$2+interval '10 seconds',$2),($1,'failed',$2,$2+interval '10 seconds',$2),($1,'succeeded',$2-interval '3 days',$2-interval '3 days',$2-interval '3 days')").bind(service).bind(at).execute(&pool).await.unwrap();
    let job: Uuid = sqlx::query_scalar(
        "INSERT INTO backup_jobs(name,target_kind) VALUES ('job','volume') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO backup_runs(job_id,status,kind,started_at,finished_at) VALUES ($1,'succeeded','backup',$2-interval '3 days',$2-interval '3 days'),($1,'partial','backup',$2,$2),($1,'failed','backup',$2,$2),($1,'succeeded','restore',$2,$2)").bind(job).bind(at).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO system_events(kind,title,created_at) VALUES ('alert.fired','fired',$1),('alert.fired','old',$1-interval '31 days')").bind(at).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mcp_calls(tool,scope,status,http_status,duration_ms,created_at) VALUES ('first','self','ok',200,1,$1),('second','self','error',500,1,$1),('old','self','error',500,1,$1-interval '31 days')").bind(at).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO domains(service_id,host,target_port,ssl_mode) VALUES ($1,'example.com',80,'none')")
        .bind(service)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO plugin_installs(plugin,service_id,installed_version,image) VALUES ('dockside',$1,'1','image')").bind(service).execute(&pool).await.unwrap();
    sqlx::query("UPDATE services SET server_id=$2 WHERE id=$1")
        .bind(service)
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
    rollup(&pool, at, "5m", b, Some(service), "requests", 2.0).await;
    rollup(&pool, at, "5m", b, Some(service), "cpu_percent", 50.0).await;
    rollup(&pool, at, "5m", b, Some(service), "restarts", 90.0).await;
    rollup(
        &pool,
        at + Duration::minutes(5),
        "1m",
        b,
        Some(service),
        "restarts",
        92.0,
    )
    .await;
    let (status, body) = get(&app, &token, "24h").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let m = &body["metrics"];
    let ops = &body["operations"];
    for (key, expected) in [
        ("rx_bytes", 90500.0),
        ("tx_bytes", 18050.0),
        ("requests", 62.0),
        ("http_5xx", 6.0),
        ("cpu_avg", 40.0),
        ("cpu_peak", 60.0),
        ("memory_avg", 50.0),
        ("memory_peak", 70.0),
    ] {
        assert_eq!(m["totals"][key].as_f64(), Some(expected), "{key}: {body}");
    }
    assert!((m["totals"]["http_5xx_rate"].as_f64().unwrap() - 6.0 / 62.0).abs() < 1e-8);
    assert_eq!(m["top_cpu"].as_array().unwrap().len(), 10);
    assert_eq!(m["top_cpu"][0]["service_id"], service.to_string());
    assert_eq!(m["top_requests"][0]["requests"].as_f64(), Some(62.0));
    assert_eq!(m["top_requests"].as_array().unwrap().len(), 1);
    assert_eq!(m["top_cpu"][0]["cpu_percent"].as_f64(), Some(74.5));
    assert_eq!(m["top_cpu"][0]["server_id"], b.to_string());
    assert_eq!(m["container_restarts"].as_f64(), Some(5.0));
    assert_eq!(m["disks"][0]["disk_percent"].as_f64(), Some(70.0));
    assert_eq!(m["disks"][1]["disk_percent"].as_f64(), Some(50.0));
    assert_eq!(ops["deploys"], 2);
    assert_eq!(ops["deploy_success_rate"].as_f64(), Some(0.5));
    assert_eq!(ops["deploy_avg_duration_seconds"].as_f64(), Some(10.0));
    assert_eq!(ops["backup_runs"], 2);
    assert_eq!(ops["backup_failures"], 2);
    assert!(ops["backup_jobs"][0]["last_success"].is_string());
    assert_eq!(ops["alerts_fired"], 1);
    assert_eq!(ops["mcp_calls"], 2);
    assert_eq!(ops["mcp_error_rate"].as_f64(), Some(0.5));
    assert_eq!(
        ops["counts"],
        json!({"servers":2,"services":12,"domains":1,"plugins":1})
    );
    let summed: f64 = m["series"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["rx_bytes"].as_f64())
        .sum();
    assert_eq!(summed, 90500.0);
}

#[sqlx::test(migrations = "./migrations")]
async fn long_ranges_use_retained_hourly_metrics(pool: PgPool) {
    let (app, token, _) = fixture(&pool).await;
    let server = server(&pool, "old").await;
    let at: DateTime<Utc> =
        sqlx::query_scalar("SELECT date_trunc('hour',now())-interval '15 days'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let service = Uuid::new_v4();
    rollup(&pool, at, "1h", server, Some(service), "requests", 3.0).await;
    rollup(&pool, at, "1h", server, Some(service), "http_5xx", 1.0).await;
    rollup(&pool, at, "1h", server, None, "net_rx_bps", 20.0).await;
    let (status, body) = get(&app, &token, "30d").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["metrics"]["totals"]["requests"].as_f64(), Some(180.0));
    assert_eq!(body["metrics"]["totals"]["http_5xx"].as_f64(), Some(60.0));
    assert_eq!(
        body["metrics"]["totals"]["rx_bytes"].as_f64(),
        Some(72000.0)
    );
    let (_, short) = get(&app, &token, "7d").await;
    assert_eq!(short["metrics"]["totals"]["requests"].as_f64(), Some(0.0));
}

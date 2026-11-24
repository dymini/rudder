//! Persistence defaults keep existing services building on the panel.

use sqlx::{PgPool, Row};
use uuid::Uuid;

#[sqlx::test(migrations = "./migrations")]
async fn builder_defaults_constraints_and_deletion_keep_deploy_targets_independent(pool: PgPool) {
    let deploy: Uuid = sqlx::query_scalar(
        "INSERT INTO servers (name, kind) VALUES ('deploy', 'local') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let builder: Uuid = sqlx::query_scalar(
        "INSERT INTO servers (name, kind) VALUES ('builder', 'remote') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let row = sqlx::query("SELECT builder_enabled, builder_max_jobs, builder_paused, builder_draining, builder_cache_limit_bytes, arch, builder_buildx, builder_nixpacks FROM servers WHERE id = $1").bind(builder).fetch_one(&pool).await.unwrap();
    assert!(!row.get::<bool, _>("builder_enabled"));
    assert_eq!(row.get::<i32, _>("builder_max_jobs"), 1);
    assert!(!row.get::<bool, _>("builder_paused"));
    assert!(!row.get::<bool, _>("builder_draining"));
    assert_eq!(
        row.get::<i64, _>("builder_cache_limit_bytes"),
        20 * 1024 * 1024 * 1024
    );
    assert!(row.get::<Option<String>, _>("arch").is_none());
    assert!(!row.get::<bool, _>("builder_buildx"));
    assert!(!row.get::<bool, _>("builder_nixpacks"));
    let project: Uuid =
        sqlx::query_scalar("INSERT INTO projects (name) VALUES ('builds') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let env: Uuid = sqlx::query_scalar(
        "INSERT INTO environments (name, project_id) VALUES ('production', $1) RETURNING id",
    )
    .bind(project)
    .fetch_one(&pool)
    .await
    .unwrap();
    let service: Uuid = sqlx::query_scalar("INSERT INTO services (name, environment_id, server_id, kind) VALUES ('app', $1, $2, 'application') RETURNING id").bind(env).bind(deploy).fetch_one(&pool).await.unwrap();
    let row = sqlx::query("SELECT build_placement, build_builder_id, build_fallback, build_test_command, build_timeout_secs FROM services WHERE id = $1").bind(service).fetch_one(&pool).await.unwrap();
    assert_eq!(row.get::<String, _>("build_placement"), "panel");
    assert!(row.get::<Option<Uuid>, _>("build_builder_id").is_none());
    assert_eq!(row.get::<String, _>("build_fallback"), "panel");
    assert!(row.get::<Option<String>, _>("build_test_command").is_none());
    assert_eq!(row.get::<i32, _>("build_timeout_secs"), 3600);
    for statement in [
        "UPDATE servers SET builder_max_jobs = 0",
        "UPDATE servers SET builder_cache_limit_bytes = -1",
        "UPDATE services SET build_placement = 'invalid'",
        "UPDATE services SET build_fallback = 'invalid'",
        "UPDATE services SET build_timeout_secs = 0",
    ] {
        assert!(
            sqlx::query(statement).execute(&pool).await.is_err(),
            "{statement}"
        );
    }
    sqlx::query("UPDATE services SET build_builder_id = $1 WHERE id = $2")
        .bind(builder)
        .bind(service)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO build_cache_entries (builder_id, service_id, cache_key, size_bytes) VALUES ($1, $2, 'branch', 12)").bind(builder).bind(service).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM servers WHERE id = $1")
        .bind(builder)
        .execute(&pool)
        .await
        .unwrap();
    let row = sqlx::query("SELECT server_id, build_builder_id FROM services WHERE id = $1")
        .bind(service)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<Uuid, _>("server_id"), deploy);
    assert!(row.get::<Option<Uuid>, _>("build_builder_id").is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM build_cache_entries")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

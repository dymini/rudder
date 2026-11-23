use rudder_core::{host_in_zone, validate_host};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use utoipa::ToSchema;

use crate::cloudflare::Cf;
use crate::error::ApiError;
use crate::routes::catalog::bad;

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct CloudflareZone {
    pub zone_id: String,
    pub account_id: String,
    pub name: String,
    pub status: String,
    pub enabled: bool,
    pub accessible: bool,
    pub last_error: Option<String>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
    pub published_hosts: Vec<String>,
}

pub async fn list(pool: &PgPool) -> Result<Vec<CloudflareZone>, ApiError> {
    sqlx::query_as("SELECT z.* FROM cloudflare_zones z JOIN cloudflare_settings s ON s.account_id=z.account_id WHERE s.id=TRUE ORDER BY z.name")
        .fetch_all(pool).await.map_err(ApiError::database)
}

pub fn resolve<'a>(zones: &'a [CloudflareZone], host: &str) -> Result<&'a CloudflareZone, ApiError> {
    let zone = lookup(zones, host)?;
    if !zone.enabled || !zone.accessible || zone.status != "active" {
        return Err(bad("zone_unavailable", format!("Cloudflare zone {} is {}. Enable it and check Zone Read and DNS Edit permissions; its parent zone will not be used.", zone.name, if !zone.enabled { "disabled" } else if !zone.accessible { "inaccessible" } else { &zone.status })));
    }
    Ok(zone)
}

pub fn lookup<'a>(zones: &'a [CloudflareZone], host: &str) -> Result<&'a CloudflareZone, ApiError> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    validate_host(&host).map_err(|_| bad("invalid_host", "Use a valid hostname without a wildcard."))?;
    let zone = zones.iter().filter(|z| host_in_zone(&host, &z.name))
        .max_by_key(|z| z.name.len())
        .ok_or_else(|| bad("zone_not_found", format!("No known Cloudflare zone matches {host}. Known zones: {}. Refresh zones and enable the matching zone.", zones.iter().map(|z|z.name.as_str()).collect::<Vec<_>>().join(", "))))?;
    Ok(zone)
}

pub async fn refresh(pool: &PgPool, client: &Cf, account: &str) -> Result<Vec<CloudflareZone>, ApiError> {
    let discovered = match client.zones(account).await {
        Ok(zones) => zones,
        Err(error) => {
            sqlx::query("UPDATE cloudflare_zones SET accessible=FALSE,last_error=$2 WHERE account_id=$1")
                .bind(account).bind(error.public_message()).execute(pool).await.map_err(ApiError::database)?;
            return Err(error);
        }
    };
    let mut tx = pool.begin().await.map_err(ApiError::database)?;
    sqlx::query("UPDATE cloudflare_zones SET accessible=FALSE,last_error='Zone is no longer visible to the connected token' WHERE account_id=$1")
        .bind(account).execute(&mut *tx).await.map_err(ApiError::database)?;
    for zone in discovered {
        sqlx::query("INSERT INTO cloudflare_zones(zone_id,account_id,name,status) VALUES($1,$2,$3,$4) ON CONFLICT(zone_id) DO UPDATE SET name=$3,status=$4,accessible=TRUE,last_error=NULL,last_seen_at=now() WHERE cloudflare_zones.account_id=$2")
            .bind(&zone.zone_id).bind(account).bind(&zone.name).bind(&zone.status)
            .execute(&mut *tx).await.map_err(ApiError::database)?;
    }
    tx.commit().await.map_err(ApiError::database)?;
    list(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(name: &str, enabled: bool) -> CloudflareZone {
        CloudflareZone { zone_id: name.into(), account_id: "account".into(), name: name.into(), status: "active".into(), enabled, accessible: true, last_error: None, last_seen_at: chrono::Utc::now(), published_hosts: vec![] }
    }

    #[test]
    fn longest_suffix_and_boundaries() {
        let zones = vec![zone("example.com", true), zone("sub.example.com", true)];
        assert_eq!(resolve(&zones, "  API.Sub.Example.Com. ").unwrap().name, "sub.example.com");
        assert_eq!(resolve(&zones, "example.com").unwrap().name, "example.com");
        for host in ["badexample.com", "example.com.evil.dev", "*.example.com", "", "https://example.com"] {
            assert!(resolve(&zones, host).is_err(), "{host}");
        }
    }

    #[test]
    fn unavailable_child_never_falls_back() {
        let mut zones = vec![zone("example.com", true), zone("sub.example.com", false)];
        assert!(resolve(&zones, "api.sub.example.com").is_err());
        zones[1].enabled = true;
        zones[1].accessible = false;
        assert!(resolve(&zones, "api.sub.example.com").is_err());
        zones[1].accessible = true;
        zones[1].status = "pending".into();
        assert!(resolve(&zones, "api.sub.example.com").is_err());
    }
}

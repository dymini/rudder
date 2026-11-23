use std::net::IpAddr;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::Json;
use futures_util::{stream, StreamExt};
use rudder_core::DnsRecord;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::cloudflare::{self as cf, Settings};
use crate::cloudflare_zones as zones;
use crate::error::ApiError;
use crate::routes::catalog::{bad, conflict, not_found};
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct DomainRoutingOptions {
    pub cloudflare_available: bool,
    pub cloudflare_reason: Option<String>,
    pub cloudflare_reason_code: Option<String>,
    pub zone_name: String,
    pub zone_id: Option<String>,
    pub direct_warning: Option<String>,
}

pub async fn options(
    state: &AppState,
    service: Uuid,
    host: &str,
) -> Result<DomainRoutingOptions, ApiError> {
    let kind: String = sqlx::query_scalar(
        "SELECT sv.kind FROM services s JOIN servers sv ON sv.id=s.server_id WHERE s.id=$1",
    )
    .bind(service)
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::database)?
    .ok_or_else(|| not_found("service not found"))?;
    let settings = cf::load(&state.pool).await?;
    let available = zones::list(&state.pool).await?;
    let resolved = zones::resolve(&available, host);
    let reason = if settings.api_token_enc.is_none() {
        Some("Save Cloudflare account credentials before choosing Cloudflare routing.".to_owned())
    } else {
        resolved.as_ref().err().map(|e| e.public_message())
    };
    Ok(DomainRoutingOptions {
        cloudflare_available: reason.is_none(),
        cloudflare_reason_code: reason.as_ref().map(|_| if settings.api_token_enc.is_none() {"credentials"} else {"zone"}.into()),
        zone_name: zones::lookup(&available,host).map(|z| z.name.clone()).unwrap_or_default(),
        zone_id: zones::lookup(&available,host).ok().map(|z| z.zone_id.clone()),
        cloudflare_reason: reason,
        direct_warning: (kind == "local" && settings.tunnel_id.is_some()).then(||
            "Direct routing requires public ports 80/443. If those ports are closed, this domain will not be reachable.".to_owned()),
    })
}

pub async fn validate(
    state: &AppState,
    service: Uuid,
    host: &str,
    mode: &str,
    id: Option<Uuid>,
) -> Result<(), ApiError> {
    if !matches!(mode, "direct" | "cloudflare") {
        return Err(bad(
            "invalid_routing",
            "routing_mode must be direct or cloudflare",
        ));
    }
    let mixed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM domains WHERE host=$1 AND id<>$2 AND routing_mode<>$3)",
    )
    .bind(host)
    .bind(id.unwrap_or(Uuid::nil()))
    .bind(mode)
    .fetch_one(&state.pool)
    .await
    .map_err(ApiError::database)?;
    if mixed {
        return Err(conflict("dns_conflict", "Every path for the same hostname must use the same routing mode because DNS applies to the whole hostname."));
    }
    if mode == "cloudflare" {
        if let Some(reason) = options(state, service, host).await?.cloudflare_reason {
            return Err(bad("cloudflare_unavailable", reason));
        }
        let row = sqlx::query("SELECT sv.kind, sv.public_ip FROM services s JOIN servers sv ON sv.id=s.server_id WHERE s.id=$1")
            .bind(service).fetch_one(&state.pool).await.map_err(ApiError::database)?;
        let settings = cf::load(&state.pool).await?;
        if !(row.get::<String, _>("kind") == "local"
            && settings.tunnel_id.is_some()
            && settings.tunnel_published)
        {
            public_ip(row.get::<Option<String>, _>("public_ip").as_deref())?;
        }
        let elsewhere: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM domains d JOIN services s ON s.id=d.service_id WHERE d.host=$1 AND d.routing_mode='cloudflare' AND s.server_id<>(SELECT server_id FROM services WHERE id=$2))")
            .bind(host).bind(service).fetch_one(&state.pool).await.map_err(ApiError::database)?;
        if elsewhere {
            return Err(conflict(
                "dns_conflict",
                "This hostname already uses Cloudflare routing on another server.",
            ));
        }
    }
    Ok(())
}

pub async fn check_conflicts(
    state: &AppState,
    host: &str,
    mode: &str,
    overwrite: bool,
) -> Result<(), ApiError> {
    if mode != "cloudflare" || overwrite {
        return Ok(());
    }
    let settings = cf::load(&state.pool).await?;
    let client = cf::client(state, &settings)?;
    let available = zones::list(&state.pool).await?;
    let zone = zones::resolve(&available, host)?;
    for record in client.dns_records(&zone.zone_id, host).await? {
        if matches!(record.kind.as_str(), "A" | "AAAA" | "CNAME")
            && !cf::owned_record_or_legacy(state, &client, &zone.zone_id, &record).await?
        {
            return Err(conflict("dns_conflict", format!("DNS for {host} is not managed by Rudder. Use overwrite_dns to explicitly replace it.")));
        }
    }
    Ok(())
}

fn public_ip(value: Option<&str>) -> Result<IpAddr, ApiError> {
    value.and_then(|v| v.parse().ok()).ok_or_else(|| bad("public_ip_required", "Set a valid server public IPv4 or IPv6 address before using Cloudflare routing without a tunnel."))
}

pub async fn remember(
    state: &AppState,
    zone: &str,
    host: &str,
    id: &str,
    kind: &str,
    content: &str,
) -> Result<(), ApiError> {
    sqlx::query("INSERT INTO domain_dns_records(zone_id,host,record_id,kind,content) VALUES($1,$2,$3,$4,$5) ON CONFLICT(zone_id,record_id) DO UPDATE SET host=$2,kind=$4,content=$5")
        .bind(zone).bind(host).bind(id).bind(kind).bind(content).execute(&state.pool).await.map_err(ApiError::database)?;
    Ok(())
}

pub async fn owned(state: &AppState, zone: &str, record: &DnsRecord) -> Result<bool, ApiError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM domain_dns_records WHERE zone_id=$1 AND record_id=$2 AND host=$3 AND kind=$4 AND lower(content)=lower($5))")
        .bind(zone).bind(&record.id).bind(&record.name).bind(&record.kind).bind(&record.content)
        .fetch_one(&state.pool).await.map_err(ApiError::database)
}

/// Reconcile only address/CNAME records; ownership includes ID and last written contents.
pub async fn reconcile(
    state: &AppState,
    overwrite: bool,
    overwrite_host: Option<&str>,
) -> Result<(), ApiError> {
    let _guard = zones::lock(&state.pool).await?;
    let settings = cf::load(&state.pool).await?;
    let results = reconcile_settings(state, settings, overwrite, overwrite_host).await?;
    if let Some(failed) = results
        .iter()
        .find(|r| matches!(r.action.as_str(), "error" | "conflict"))
    {
        return Err(conflict(
            "dns_conflict",
            format!(
                "{} ({}): {}",
                failed.host,
                failed.zone_name,
                failed
                    .detail
                    .as_deref()
                    .unwrap_or("DNS reconciliation failed")
            ),
        ));
    }
    Ok(())
}

pub async fn reconcile_tunnel(
    state: &AppState,
    overwrite: bool,
    overwrite_host: Option<&str>,
) -> Result<Vec<cf::DnsOutcome>, ApiError> {
    let mut settings = cf::load(&state.pool).await?;
    settings.tunnel_published = true;
    reconcile_settings(state, settings, overwrite, overwrite_host).await
}

async fn reconcile_settings(
    state: &AppState,
    settings: Settings,
    overwrite: bool,
    overwrite_host: Option<&str>,
) -> Result<Vec<cf::DnsOutcome>, ApiError> {
    if settings.api_token_enc.is_none() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query("SELECT DISTINCT d.host, sv.kind, sv.public_ip FROM domains d JOIN services s ON s.id=d.service_id JOIN servers sv ON sv.id=s.server_id WHERE d.routing_mode='cloudflare'")
        .fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let tunnel = settings
        .tunnel_id
        .as_deref()
        .filter(|_| settings.tunnel_published);
    let mut desired = Vec::new();
    let available = zones::list(&state.pool).await?;
    let mut results = Vec::new();
    let mut all_hosts = std::collections::HashSet::new();
    for row in rows {
        let host: String = row.get("host");
        all_hosts.insert(host.clone());
        let zone = match zones::resolve(&available, &host) {
            Ok(zone) => zone,
            Err(_) => continue,
        };
        let (kind, content) = if row.get::<String, _>("kind") == "local" && tunnel.is_some() {
            (
                "CNAME",
                rudder_core::tunnel_cname(tunnel.unwrap_or_default()),
            )
        } else {
            let ip = match public_ip(row.get::<Option<String>, _>("public_ip").as_deref()) {
                Ok(ip) => ip,
                Err(error) => {
                    results.push(cf::DnsOutcome {
                        host,
                        zone_id: zone.zone_id.clone(),
                        zone_name: zone.name.clone(),
                        action: "error".into(),
                        detail: Some(error.public_message()),
                    });
                    continue;
                }
            };
            (if ip.is_ipv4() { "A" } else { "AAAA" }, ip.to_string())
        };
        desired.push((host, zone, kind, content));
    }
    let keep_tunnel_hosts = if tunnel.is_some() {
        cf::wanted_hosts(&state.pool)
            .await?
            .0
            .into_iter()
            .map(|h| h.host)
            .collect()
    } else {
        Vec::new()
    };
    let managed =
        sqlx::query("SELECT r.zone_id,r.host,r.record_id,r.kind,r.content FROM domain_dns_records r JOIN cloudflare_zones z ON z.zone_id=r.zone_id WHERE z.account_id=$1 AND z.enabled AND z.accessible AND z.status='active'")
            .bind(&settings.account_id)
            .fetch_all(&state.pool)
            .await
            .map_err(ApiError::database)?;
    let client = cf::client(state, &settings)?;
    for row in managed {
        let host: String = row.get("host");
        let zone_id: String = row.get("zone_id");
        let keep = all_hosts.contains(&host)
            || zones::resolve(&available, &host).is_err()
            || (row.get::<String, _>("kind") == "CNAME"
                && tunnel.is_some_and(|id| {
                    row.get::<String, _>("content") == rudder_core::tunnel_cname(id)
                })
                && keep_tunnel_hosts.contains(&host));
        if !keep {
            let cleanup = async {
                for record in client.dns_records(&zone_id, &host).await? {
                    if record.id == row.get::<String, _>("record_id")
                        && owned(state, &zone_id, &record).await?
                    {
                        client.delete_dns(&zone_id, &record.id).await?;
                    }
                }
                sqlx::query("DELETE FROM domain_dns_records WHERE zone_id=$1 AND record_id=$2")
                    .bind(&zone_id)
                    .bind(row.get::<String, _>("record_id"))
                    .execute(&state.pool)
                    .await
                    .map_err(ApiError::database)?;
                Ok::<_, ApiError>(())
            }
            .await;
            if let Err(error) = cleanup {
                results.push(cf::DnsOutcome {
                    host,
                    zone_id: zone_id.clone(),
                    zone_name: available
                        .iter()
                        .find(|z| z.zone_id == zone_id)
                        .map(|z| z.name.clone())
                        .unwrap_or_default(),
                    action: "error".into(),
                    detail: Some(error.public_message()),
                });
            }
        }
    }
    for (host, zone, kind, content) in desired {
        if kind == "CNAME" {
            continue;
        }
        let result = reconcile_address(
            state,
            &client,
            &zone.zone_id,
            &host,
            kind,
            &content,
            overwrite && overwrite_host.is_none_or(|allowed| allowed == host),
        )
        .await;
        results.push(cf::DnsOutcome {
            host,
            zone_id: zone.zone_id.clone(),
            zone_name: zone.name.clone(),
            action: result.as_ref().copied().unwrap_or("error").into(),
            detail: result.err().map(|e| e.public_message()),
        });
    }
    Ok(results)
}

async fn reconcile_address(
    state: &AppState,
    client: &cf::Cf,
    zone: &str,
    host: &str,
    kind: &str,
    content: &str,
    overwrite: bool,
) -> Result<&'static str, ApiError> {
    let records = client.dns_records(zone, host).await?;
    let addresses: Vec<_> = records
        .iter()
        .filter(|r| matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME"))
        .collect();
    for record in &addresses {
        if !owned(state, zone, record).await? && !overwrite {
            return Err(conflict("dns_conflict",format!("DNS for {host} is not managed by Rudder. Use overwrite_dns to explicitly replace it.")));
        }
    }
    let matching = addresses
        .iter()
        .find(|r| r.kind == kind && r.content.eq_ignore_ascii_case(content));
    let (id, action) = if let Some(record) = matching {
        (record.id.clone(), "kept")
    } else if let Some(record) = addresses.first() {
        client
            .replace_dns(zone, &record.id, host, kind, content)
            .await?;
        (record.id.clone(), "replaced")
    } else {
        (
            client
                .create_address(zone, host, public_ip(Some(content))?)
                .await?,
            "created",
        )
    };
    remember(state, zone, host, &id, kind, content).await?;
    for record in addresses.iter().filter(|r| r.id != id) {
        client.delete_dns(zone, &record.id).await?;
        sqlx::query("DELETE FROM domain_dns_records WHERE zone_id=$1 AND record_id=$2")
            .bind(zone)
            .bind(&record.id)
            .execute(&state.pool)
            .await
            .map_err(ApiError::database)?;
    }
    Ok(action)
}

#[derive(Deserialize)]
pub struct RoutingQuery {
    pub service_id: Uuid,
    pub host: String,
}

pub async fn routing_options(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<RoutingQuery>,
) -> Result<Json<DomainRoutingOptions>, ApiError> {
    Ok(Json(options(&state, query.service_id, &query.host).await?))
}

#[derive(Serialize, ToSchema)]
pub struct DnsCheck {
    pub addresses: Vec<String>,
    pub expected: Option<String>,
    pub matches_expected: Option<bool>,
    pub message: String,
}

#[derive(Serialize, ToSchema)]
pub struct GlobalDomainView {
    pub zone_id: Option<String>,
    pub zone_name: Option<String>,
    pub zone_error: Option<String>,
    pub id: Option<Uuid>,
    pub host: String,
    pub path_prefix: String,
    pub kind: String,
    pub service_id: Option<Uuid>,
    pub service_name: Option<String>,
    pub service_kind: Option<String>,
    pub server_id: Option<Uuid>,
    pub server_name: Option<String>,
    pub routing_mode: String,
    pub routing: String,
    pub ssl_mode: Option<String>,
    pub certificate_status: Option<String>,
    pub routing_options: Option<DomainRoutingOptions>,
    pub dns: DnsCheck,
}

fn in_network(address: &str, cidr: &str) -> bool {
    let Some((network, bits)) = cidr.split_once('/') else {
        return false;
    };
    match (
        address.parse::<IpAddr>(),
        network.parse::<IpAddr>(),
        bits.parse::<u32>(),
    ) {
        (Ok(IpAddr::V4(ip)), Ok(IpAddr::V4(net)), Ok(bits)) if bits <= 32 => {
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(ip) & mask == u32::from(net) & mask
        }
        (Ok(IpAddr::V6(ip)), Ok(IpAddr::V6(net)), Ok(bits)) if bits <= 128 => {
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(ip) & mask == u128::from(net) & mask
        }
        _ => false,
    }
}

fn pending_dns(expected: Option<String>) -> DnsCheck {
    DnsCheck {
        addresses: Vec::new(),
        expected,
        matches_expected: None,
        message: String::new(),
    }
}

async fn dns_check(
    host: &str,
    expected: Option<String>,
    proxied: bool,
    client: Option<&cf::Cf>,
    available: &[zones::CloudflareZone],
    networks: &[String],
) -> DnsCheck {
    let expected = expected.map(|value| {
        value
            .parse::<IpAddr>()
            .map(|ip| ip.to_string())
            .unwrap_or(value)
    });
    let result =
        tokio::time::timeout(Duration::from_secs(2), tokio::net::lookup_host((host, 443))).await;
    let mut addresses: Vec<String> = match result {
        Ok(Ok(values)) => values.map(|v| v.ip().to_string()).collect(),
        _ => Vec::new(),
    };
    addresses.sort();
    addresses.dedup();
    let matched = if addresses.is_empty() {
        Some(false)
    } else if proxied {
        match (
            client,
            expected.as_deref(),
            zones::resolve(available, host).ok(),
        ) {
            (Some(client), Some(target), Some(zone)) if !networks.is_empty() => {
                match tokio::time::timeout(
                    Duration::from_secs(2),
                    client.proxied_target(&zone.zone_id, host, target),
                )
                .await
                {
                    Ok(Ok(origin_matches)) => Some(
                        origin_matches
                            && addresses
                                .iter()
                                .all(|ip| networks.iter().any(|cidr| in_network(ip, cidr))),
                    ),
                    _ => None,
                }
            }
            _ => None,
        }
    } else {
        expected.as_ref().map(|ip| addresses.contains(ip))
    };
    let message = if addresses.is_empty() {
        "DNS did not resolve within two seconds."
    } else if proxied && matched == Some(true) {
        "DNS resolves through Cloudflare and the origin target matches."
    } else if proxied && matched == Some(false) {
        "DNS or the Cloudflare origin target does not match the expected route."
    } else if proxied {
        "DNS resolves, but the Cloudflare origin could not be verified."
    } else if matched == Some(true) {
        "DNS points to the server public IP."
    } else if expected.is_none() {
        "Set the server public IP to check the DNS target."
    } else {
        "DNS does not point to the server public IP."
    };
    DnsCheck {
        addresses,
        expected,
        matches_expected: matched,
        message: message.into(),
    }
}

pub async fn inventory(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Vec<GlobalDomainView>>, ApiError> {
    let settings: Settings = cf::load(&state.pool).await?;
    let available = zones::list(&state.pool).await?;
    let rows = sqlx::query("SELECT d.id,d.host,d.path_prefix,d.service_id,d.routing_mode,d.ssl_mode, s.name service_name,s.kind service_kind,sv.id server_id,sv.name server_name,sv.kind server_kind,sv.public_ip, c.not_after FROM domains d JOIN services s ON s.id=d.service_id JOIN servers sv ON sv.id=s.server_id LEFT JOIN certificates c ON c.id=d.certificate_id ORDER BY d.host,d.path_prefix")
        .fetch_all(&state.pool).await.map_err(ApiError::database)?;
    let mut views = Vec::new();
    for row in rows {
        let service: Uuid = row.get("service_id");
        let host: String = row.get("host");
        let mode: String = row.get("routing_mode");
        let tunnel = mode == "cloudflare"
            && row.get::<String, _>("server_kind") == "local"
            && settings.tunnel_published
            && settings.tunnel_id.is_some();
        let routing = if tunnel {
            "tunnel"
        } else if mode == "cloudflare" {
            "cloudflare-proxied"
        } else {
            "direct"
        };
        let expiry: Option<chrono::DateTime<chrono::Utc>> = row.get("not_after");
        let ssl: String = row.get("ssl_mode");
        views.push(GlobalDomainView {
            zone_id: zones::lookup(&available, &host)
                .ok()
                .map(|z| z.zone_id.clone()),
            zone_name: zones::lookup(&available, &host)
                .ok()
                .map(|z| z.name.clone()),
            zone_error: zones::resolve(&available, &host)
                .err()
                .map(|e| e.public_message()),
            id: Some(row.get("id")),
            host: host.clone(),
            path_prefix: row.get("path_prefix"),
            kind: "service".into(),
            service_id: Some(service),
            service_name: Some(row.get("service_name")),
            service_kind: Some(row.get("service_kind")),
            server_id: Some(row.get("server_id")),
            server_name: Some(row.get("server_name")),
            routing_mode: mode,
            routing: routing.into(),
            ssl_mode: Some(ssl),
            certificate_status: expiry.map(|at| {
                if at < chrono::Utc::now() {
                    "expired".into()
                } else {
                    "valid".into()
                }
            }),
            routing_options: Some(options(&state, service, &host).await?),
            dns: pending_dns(if tunnel {
                settings.tunnel_id.as_deref().map(rudder_core::tunnel_cname)
            } else {
                row.get("public_ip")
            }),
        });
    }
    let panel = sqlx::query("SELECT panel_domain,panel_ssl_mode FROM panel_settings WHERE id=TRUE")
        .fetch_one(&state.pool)
        .await
        .map_err(ApiError::database)?;
    let portal: Option<String> =
        sqlx::query_scalar("SELECT portal_host FROM access_settings WHERE id=TRUE")
            .fetch_one(&state.pool)
            .await
            .map_err(ApiError::database)?;
    let local = sqlx::query(
        "SELECT id,name,public_ip FROM servers WHERE kind='local' ORDER BY created_at LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(ApiError::database)?;
    for (kind, host, ssl) in [
        (
            "panel",
            panel.get::<Option<String>, _>("panel_domain"),
            Some(panel.get::<String, _>("panel_ssl_mode")),
        ),
        ("access", portal, None),
    ] {
        if let Some(host) = host.filter(|h| !h.is_empty()) {
            let tunnel = settings.tunnel_published && settings.published_hosts.contains(&host);
            let ip: Option<String> = local.as_ref().and_then(|r| r.get("public_ip"));
            views.push(GlobalDomainView {
                zone_id: zones::lookup(&available, &host)
                    .ok()
                    .map(|z| z.zone_id.clone()),
                zone_name: zones::lookup(&available, &host)
                    .ok()
                    .map(|z| z.name.clone()),
                zone_error: zones::resolve(&available, &host)
                    .err()
                    .map(|e| e.public_message()),
                id: None,
                host: host.clone(),
                path_prefix: "/".into(),
                kind: kind.into(),
                service_id: None,
                service_name: None,
                service_kind: None,
                server_id: local.as_ref().map(|r| r.get("id")),
                server_name: local.as_ref().map(|r| r.get("name")),
                routing_mode: if tunnel { "cloudflare" } else { "direct" }.into(),
                routing: if tunnel { "tunnel" } else { "direct" }.into(),
                ssl_mode: ssl,
                certificate_status: None,
                routing_options: None,
                dns: pending_dns(if tunnel {
                    settings.tunnel_id.as_deref().map(rudder_core::tunnel_cname)
                } else {
                    ip
                }),
            });
        }
    }
    let client = cf::client(&state, &settings).ok();
    let networks = if let Some(client) = &client {
        tokio::time::timeout(Duration::from_secs(2), client.proxy_networks())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let views = stream::iter(views)
        .map(|mut view| {
            let client = client.as_ref();
            let available = &available;
            let networks = &networks;
            async move {
                view.dns = dns_check(
                    &view.host,
                    view.dns.expected,
                    view.routing != "direct",
                    client,
                    available,
                    networks,
                )
                .await;
                view
            }
        })
        .buffered(16)
        .collect::<Vec<_>>()
        .await;
    Ok(Json(views))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_checks_handle_both_families_and_bad_input() {
        assert!(in_network("173.245.48.5", "173.245.48.0/20"));
        assert!(!in_network("192.0.2.1", "173.245.48.0/20"));
        assert!(in_network("2606:4700::1", "2606:4700::/32"));
        assert!(!in_network("2606:4700::1", "192.0.2.0/24"));
        assert!(!in_network("127.0.0.1", "127.0.0.1/33"));
        assert!(!in_network("", ""));
        assert!(!in_network("bad", "bad/1"));
    }
}

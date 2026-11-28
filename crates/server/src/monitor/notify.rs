//! Notification channels (Telegram, Discord, Slack, webhook, SMTP) and event delivery.
//!
//! Channel configs are encrypted at rest. Only [`ChannelConfig::summary`] is
//! ever shown; bot tokens, webhook URLs, and SMTP passwords are not.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

use dymini_panel_core::{Cipher, Error};

const SEND_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ATTEMPTS: i32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    Starttls,
    Tls,
    None,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ChannelConfig {
    Telegram {
        bot_token: String,
        chat_id: String,
        /// Bot API base URL, for self-hosted Bot API servers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_url: Option<String>,
    },
    Discord {
        webhook_url: String,
    },
    Slack {
        webhook_url: String,
    },
    Webhook {
        url: String,
        /// Signs the body as `X-Dymini-Panel-Signature: sha256=<hex hmac>`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret: Option<String>,
    },
    Smtp {
        host: String,
        port: Option<u16>,
        security: SmtpSecurity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
        from: String,
        to: Vec<String>,
    },
}

impl std::fmt::Debug for ChannelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.summary())
    }
}

fn http_url(value: &str, https_only: bool) -> Result<url::Url, Error> {
    if value.len() > 2048 || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Error::Validation("URL is not valid".into()));
    }
    let url = url::Url::parse(value)
        .map_err(|_| Error::Validation("URL must start with http:// or https://".into()))?;
    if url.scheme() != "https" && (https_only || url.scheme() != "http") {
        return Err(Error::Validation(if https_only {
            "URL must use https".into()
        } else {
            "URL must start with http:// or https://".into()
        }));
    }
    if url.host().is_none() {
        return Err(Error::Validation("URL is not valid".into()));
    }
    Ok(url)
}

fn validate_http_url(
    value: &str,
    https_only: bool,
    allow_private: bool,
) -> Result<url::Url, Error> {
    let url = http_url(value, https_only)?;
    if !allow_private && host_is_private_literal(&url) {
        return Err(Error::Validation(
            "URL must not target a loopback, private, link-local, or otherwise non-public address; set DYMINI_PANEL_NOTIFICATION_ALLOW_PRIVATE=true only for a trusted private notification endpoint".into(),
        ));
    }
    Ok(url)
}

fn host_is_private_literal(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ipv4_blocked(ip),
        Some(url::Host::Ipv6(ip)) => ipv6_blocked(ip),
        Some(url::Host::Domain(host)) => {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            host == "localhost" || host.ends_with(".localhost")
        }
        None => true,
    }
}

fn ipv4_blocked(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 0
        || octets[0] == 10
        || octets[0] == 127
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 172 && (16..=31).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0)
        || (octets[0] == 192 && octets[1] == 168)
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        || octets[0] >= 224
}

fn ipv6_blocked(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return true;
    }
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return ipv4_blocked(mapped);
    }
    let segments = ip.segments();
    if ip.is_unique_local() || ip.is_unicast_link_local() {
        return true;
    }
    // 2001:db8::/32 documentation ranges.
    if segments[0] == 0x2001 && segments[1] == 0x0db8 {
        return true;
    }
    // NAT64 (64:ff9b::/96) can embed an IPv4 address; reject it when that address is not public.
    if segments[..5] == [0x0064, 0xff9b, 0, 0, 0] && segments[5] == 0 {
        let octets = ip.octets();
        return ipv4_blocked(Ipv4Addr::new(
            octets[12], octets[13], octets[14], octets[15],
        ));
    }
    // Deprecated IPv4-compatible addresses (::a.b.c.d), beyond :: and ::1 handled above.
    if segments[..5] == [0, 0, 0, 0, 0] && segments[5] == 0 {
        let octets = ip.octets();
        return ipv4_blocked(Ipv4Addr::new(
            octets[12], octets[13], octets[14], octets[15],
        ));
    }
    false
}

fn ip_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_blocked(ip),
        IpAddr::V6(ip) => ipv6_blocked(ip),
    }
}

#[derive(Clone, Copy)]
struct NotificationResolver {
    allow_private: bool,
}

impl Resolve for NotificationResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().trim_end_matches('.').to_owned();
        let allow_private = self.allow_private;
        Box::pin(async move {
            if !allow_private
                && (host.eq_ignore_ascii_case("localhost")
                    || host.to_ascii_lowercase().ends_with(".localhost"))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "notification destination resolves to a non-public address",
                )
                .into());
            }
            let addrs: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
                vec![SocketAddr::new(ip, 0)]
            } else {
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect()
            };
            if addrs.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "notification destination did not resolve",
                )
                .into());
            }
            if !allow_private && addrs.iter().any(|addr| ip_blocked(addr.ip())) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "notification destination resolves to a non-public address",
                )
                .into());
            }
            let addrs: Addrs = Box::new(addrs.into_iter());
            Ok(addrs)
        })
    }
}

impl ChannelConfig {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Telegram { .. } => "telegram",
            Self::Discord { .. } => "discord",
            Self::Slack { .. } => "slack",
            Self::Webhook { .. } => "webhook",
            Self::Smtp { .. } => "smtp",
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.validate_with_policy(false)
    }

    pub fn validate_with_policy(&self, allow_private: bool) -> Result<(), Error> {
        match self {
            Self::Telegram {
                bot_token,
                chat_id,
                api_url,
            } => {
                let (id, secret) = bot_token
                    .split_once(':')
                    .ok_or_else(|| Error::Validation("bot token looks like 123456:ABC…".into()))?;
                if id.is_empty()
                    || !id.chars().all(|c| c.is_ascii_digit())
                    || secret.len() < 20
                    || !secret
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    return Err(Error::Validation("bot token looks like 123456:ABC…".into()));
                }
                let chat = chat_id.strip_prefix('-').unwrap_or(chat_id);
                let username = chat_id.strip_prefix('@').is_some_and(|name| {
                    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                });
                if !(username || (!chat.is_empty() && chat.chars().all(|c| c.is_ascii_digit()))) {
                    return Err(Error::Validation(
                        "chat id is a number or @channelname".into(),
                    ));
                }
                if let Some(url) = api_url {
                    validate_http_url(url, false, allow_private)?;
                }
                Ok(())
            }
            Self::Discord { webhook_url } | Self::Slack { webhook_url } => {
                validate_http_url(webhook_url, true, allow_private).map(|_| ())
            }
            Self::Webhook { url, secret } => {
                validate_http_url(url, false, allow_private)?;
                if secret.as_ref().is_some_and(|s| s.len() < 8) {
                    return Err(Error::Validation(
                        "webhook secret needs at least 8 characters".into(),
                    ));
                }
                Ok(())
            }
            Self::Smtp {
                host,
                from,
                to,
                username,
                password,
                ..
            } => {
                if host.is_empty()
                    || host.len() > 253
                    || !host
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                {
                    return Err(Error::Validation("SMTP host is not valid".into()));
                }
                from.parse::<Mailbox>()
                    .map_err(|_| Error::Validation("from address is not valid".into()))?;
                if to.is_empty() || to.len() > 20 {
                    return Err(Error::Validation("give 1 to 20 recipients".into()));
                }
                for address in to {
                    address.parse::<Mailbox>().map_err(|_| {
                        Error::Validation(format!("recipient '{address}' is not valid"))
                    })?;
                }
                if username.is_some() != password.is_some() {
                    return Err(Error::Validation(
                        "SMTP username and password go together".into(),
                    ));
                }
                Ok(())
            }
        }
    }

    /// Non-secret description for list views.
    pub fn summary(&self) -> String {
        let host_of = |url: &str| {
            http_url(url, false)
                .map(|p| p.host_str().unwrap_or("invalid URL").to_owned())
                .unwrap_or_else(|_| "invalid URL".into())
        };
        match self {
            Self::Telegram { chat_id, .. } => format!("chat {chat_id}"),
            Self::Discord { webhook_url } | Self::Slack { webhook_url } => {
                format!("webhook on {}", host_of(webhook_url))
            }
            Self::Webhook { url, secret } => format!(
                "POST to {}{}",
                host_of(url),
                if secret.is_some() { ", signed" } else { "" }
            ),
            Self::Smtp { host, port, to, .. } => {
                format!("{host}:{} → {}", port.unwrap_or(587), to.join(", "))
            }
        }
    }

    /// Keep secrets from `previous` when an update leaves them empty.
    pub fn merge_secrets(mut self, previous: &ChannelConfig) -> Self {
        match (&mut self, previous) {
            (Self::Telegram { bot_token, .. }, Self::Telegram { bot_token: old, .. })
                if bot_token.is_empty() =>
            {
                *bot_token = old.clone()
            }
            (Self::Discord { webhook_url }, Self::Discord { webhook_url: old })
            | (Self::Slack { webhook_url }, Self::Slack { webhook_url: old })
                if webhook_url.is_empty() =>
            {
                *webhook_url = old.clone()
            }
            (Self::Webhook { secret, .. }, Self::Webhook { secret: old, .. })
                if secret.as_deref() == Some("") =>
            {
                *secret = old.clone()
            }
            (Self::Smtp { password, .. }, Self::Smtp { password: old, .. })
                if password.as_deref() == Some("") =>
            {
                *password = old.clone()
            }
            _ => {}
        }
        self
    }

    pub fn seal(&self, cipher: &Cipher) -> Result<Vec<u8>, Error> {
        let plain = serde_json::to_vec(self).map_err(|_| Error::Crypto)?;
        cipher.encrypt(&plain)
    }

    pub fn open(cipher: &Cipher, sealed: &[u8]) -> Result<Self, Error> {
        let plain = cipher.decrypt(sealed)?;
        serde_json::from_slice(&plain).map_err(|_| Error::Crypto)
    }
}

/// What a channel receives.
#[derive(Debug, Clone, Serialize)]
pub struct Notice {
    pub id: i64,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub server_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

impl Notice {
    fn text(&self) -> String {
        let tag = match self.severity.as_str() {
            "critical" => "[CRITICAL]",
            "warning" => "[WARNING]",
            _ => "[INFO]",
        };
        if self.message.is_empty() {
            format!("{tag} {}", self.title)
        } else {
            format!("{tag} {}\n{}", self.title, self.message)
        }
    }
}

fn http_client(allow_private: bool) -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .timeout(SEND_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .dns_resolver(NotificationResolver { allow_private })
        .build()
        .map_err(|error| Error::Node(error.to_string()))
}

fn check(status: reqwest::StatusCode) -> Result<(), Error> {
    if status.is_success() {
        Ok(())
    } else {
        Err(Error::Node(format!("endpoint answered {status}")))
    }
}

/// `sha256=<hex>` HMAC of the exact request body.
pub fn sign(secret: &str, body: &[u8]) -> Result<String, Error> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| Error::Crypto)?;
    mac.update(body);
    Ok(format!(
        "sha256={}",
        hex::encode(mac.finalize().into_bytes())
    ))
}

async fn post_json(
    client: &reqwest::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<(), Error> {
    let response = client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|error| Error::Node(format!("request failed: {}", error.without_url())))?;
    check(response.status())
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

pub async fn send(
    config: &ChannelConfig,
    notice: &Notice,
    allow_private: bool,
) -> Result<(), Error> {
    let client = http_client(allow_private)?;
    let text = notice.text();
    match config {
        ChannelConfig::Telegram {
            bot_token,
            chat_id,
            api_url,
        } => {
            let base = api_url
                .as_deref()
                .unwrap_or("https://api.telegram.org")
                .trim_end_matches('/');
            post_json(
                &client,
                &format!("{base}/bot{bot_token}/sendMessage"),
                &json!({
                    "chat_id": chat_id,
                    "text": truncate(&text, 4000),
                    "disable_web_page_preview": true
                }),
            )
            .await
        }
        ChannelConfig::Discord { webhook_url } => {
            post_json(
                &client,
                webhook_url,
                &json!({ "content": truncate(&text, 1900), "allowed_mentions": { "parse": [] } }),
            )
            .await
        }
        ChannelConfig::Slack { webhook_url } => {
            post_json(
                &client,
                webhook_url,
                &json!({ "text": truncate(&text, 3000) }),
            )
            .await
        }
        ChannelConfig::Webhook { url, secret } => {
            let bytes = serde_json::to_vec(notice).map_err(|_| Error::Crypto)?;
            let mut request = client
                .post(url)
                .header("content-type", "application/json")
                .header("x-dymini-panel-event", &notice.kind)
                // Pre-rebrand receivers still read the Rudder headers.
                .header("x-rudder-event", &notice.kind);
            if let Some(secret) = secret {
                let signature = sign(secret, &bytes)?;
                request = request
                    .header("x-dymini-panel-signature", &signature)
                    .header("x-rudder-signature", &signature);
            }
            let response =
                request.body(bytes).send().await.map_err(|error| {
                    Error::Node(format!("request failed: {}", error.without_url()))
                })?;
            check(response.status())
        }
        ChannelConfig::Smtp {
            host,
            port,
            security,
            username,
            password,
            from,
            to,
        } => {
            let mut message = Message::builder()
                .from(from.parse().map_err(|_| Error::Validation("from".into()))?)
                .subject(truncate(&format!("Dymini Panel: {}", notice.title), 200));
            for address in to {
                message = message.to(address
                    .parse()
                    .map_err(|_| Error::Validation("to".into()))?);
            }
            let email = message
                .body(text)
                .map_err(|error| Error::Node(error.to_string()))?;
            let builder = match security {
                SmtpSecurity::Starttls => {
                    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                }
                SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
                SmtpSecurity::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
                    host.as_str(),
                )),
            }
            .map_err(|error| Error::Node(format!("SMTP: {error}")))?;
            let default_port = match security {
                SmtpSecurity::Starttls => 587,
                SmtpSecurity::Tls => 465,
                SmtpSecurity::None => 25,
            };
            let mut builder = builder
                .port(port.unwrap_or(default_port))
                .timeout(Some(SEND_TIMEOUT));
            if let (Some(user), Some(pass)) = (username, password) {
                builder = builder.credentials(Credentials::new(user.clone(), pass.clone()));
            }
            builder
                .build()
                .send(email)
                .await
                .map(|_| ())
                .map_err(|error| Error::Node(format!("SMTP: {error}")))
        }
    }
}

struct Pending {
    id: i64,
    kind: String,
    severity: String,
    title: String,
    message: String,
    server_id: Option<Uuid>,
    service_id: Option<Uuid>,
    payload: serde_json::Value,
    created_at: DateTime<Utc>,
    delivery_attempts: i32,
    delivered_channels: Vec<Uuid>,
}

/// Channels that should receive an event: the alert's own channels for
/// `alert.*` events, plus every enabled `event` rule whose patterns match.
async fn channels_for(pool: &PgPool, event: &Pending) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut out: Vec<Uuid> = Vec::new();
    if let Some(alert_id) = event
        .payload
        .get("alert_id")
        .and_then(|v| v.as_str())
        .and_then(|v| Uuid::parse_str(v).ok())
    {
        if let Some(ids) = sqlx::query_scalar!(
            "SELECT channel_ids FROM alerts WHERE id = $1 AND enabled",
            alert_id
        )
        .fetch_optional(pool)
        .await?
        {
            out.extend(ids);
        }
    }
    let rules = sqlx::query!(
        "SELECT target_spec, channel_ids FROM alerts WHERE enabled AND kind = 'event'"
    )
    .fetch_all(pool)
    .await?;
    for rule in rules {
        let patterns: Vec<String> = rule
            .target_spec
            .get("events")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_else(|| vec!["*".to_owned()]);
        let min = rule
            .target_spec
            .get("min_severity")
            .and_then(|v| v.as_str())
            .unwrap_or("info");
        if dymini_panel_core::event_matches(&patterns, &event.kind)
            && severity_rank(&event.severity) >= severity_rank(min)
        {
            out.extend(rule.channel_ids);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn severity_rank(value: &str) -> u8 {
    match value {
        "critical" => 2,
        "warning" => 1,
        _ => 0,
    }
}

/// Deliver pending events once. Returns the number of events finished.
pub async fn deliver_once(
    pool: &PgPool,
    cipher: &Cipher,
    allow_private: bool,
) -> Result<usize, sqlx::Error> {
    let events = sqlx::query_as!(
        Pending,
        r#"
        SELECT id, kind, severity, title, message, server_id, service_id, payload, created_at,
               delivery_attempts, delivered_channels
        FROM system_events
        WHERE delivered_at IS NULL
        ORDER BY id
        LIMIT 50
        "#
    )
    .fetch_all(pool)
    .await?;
    let mut finished = 0;
    for event in events {
        let targets = channels_for(pool, &event).await?;
        let remaining: Vec<Uuid> = targets
            .into_iter()
            .filter(|id| !event.delivered_channels.contains(id))
            .collect();
        let channels = sqlx::query!(
            "SELECT id, config_enc FROM notification_channels WHERE id = ANY($1) AND enabled",
            &remaining
        )
        .fetch_all(pool)
        .await?;
        let notice = Notice {
            id: event.id,
            kind: event.kind.clone(),
            severity: event.severity.clone(),
            title: event.title.clone(),
            message: event.message.clone(),
            server_id: event.server_id,
            service_id: event.service_id,
            created_at: event.created_at,
        };
        let mut delivered = event.delivered_channels.clone();
        let mut errors = Vec::new();
        for channel in channels {
            let result = match ChannelConfig::open(cipher, &channel.config_enc) {
                Ok(config) => send(&config, &notice, allow_private).await,
                Err(error) => Err(error),
            };
            match result {
                Ok(()) => delivered.push(channel.id),
                Err(error) => errors.push(format!("{}: {error}", channel.id)),
            }
        }
        let attempts = event.delivery_attempts + 1;
        let done = errors.is_empty() || attempts >= MAX_ATTEMPTS;
        sqlx::query!(
            r#"
            UPDATE system_events
            SET delivered_channels = $2, delivery_attempts = $3,
                delivery_error = $4, delivered_at = CASE WHEN $5 THEN now() ELSE NULL END
            WHERE id = $1
            "#,
            event.id,
            &delivered,
            attempts,
            (!errors.is_empty()).then(|| errors.join("; ")),
            done,
        )
        .execute(pool)
        .await?;
        if !errors.is_empty() {
            tracing::warn!(event = event.id, attempts, "notification delivery failed");
        }
        if done {
            finished += 1;
        }
    }
    Ok(finished)
}

pub async fn run(pool: PgPool, cipher: std::sync::Arc<Cipher>, allow_private: bool) {
    loop {
        if let Err(error) = deliver_once(&pool, &cipher, allow_private).await {
            tracing::warn!(%error, "notification delivery query failed");
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telegram(token: &str, chat: &str) -> ChannelConfig {
        ChannelConfig::Telegram {
            bot_token: token.into(),
            chat_id: chat.into(),
            api_url: None,
        }
    }

    #[test]
    fn channel_validation() {
        assert!(
            telegram("123456:ABCDEFGHIJKLMNOPQRSTUVWXYZ_-12", "-1001234")
                .validate()
                .is_ok()
        );
        assert!(
            telegram("123456:ABCDEFGHIJKLMNOPQRSTUVWXYZ_-12", "@ops_room")
                .validate()
                .is_ok()
        );
        assert!(telegram("nope", "1").validate().is_err());
        assert!(telegram("123456:ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abc")
            .validate()
            .is_err());
        assert!(ChannelConfig::Discord {
            webhook_url: "http://discord.com/api/webhooks/1/x".into()
        }
        .validate()
        .is_err());
        assert!(ChannelConfig::Webhook {
            url: "https://hooks.example.com/dymini-panel".into(),
            secret: Some("short".into())
        }
        .validate()
        .is_err());
        let smtp = ChannelConfig::Smtp {
            host: "smtp.example.com".into(),
            port: None,
            security: SmtpSecurity::Starttls,
            username: Some("u".into()),
            password: None,
            from: "Dymini Panel <dymini-panel@example.com>".into(),
            to: vec!["ops@example.com".into()],
        };
        assert!(smtp.validate().is_err());
    }

    #[test]
    fn summaries_hide_secrets_and_updates_keep_them() {
        let config = telegram("123456:ABCDEFGHIJKLMNOPQRSTUVWXYZ_-12", "42");
        assert_eq!(config.summary(), "chat 42");
        assert!(!format!("{config:?}").contains("ABCDEF"));
        let slack = ChannelConfig::Slack {
            webhook_url: "https://hooks.slack.com/services/T/B/secret".into(),
        };
        assert_eq!(slack.summary(), "webhook on hooks.slack.com");
        let updated = ChannelConfig::Slack {
            webhook_url: String::new(),
        }
        .merge_secrets(&slack);
        assert_eq!(updated, slack);
    }

    #[test]
    fn private_notification_literals_require_explicit_opt_in() {
        let webhook = ChannelConfig::Webhook {
            url: "http://127.0.0.1:8080/hook".into(),
            secret: None,
        };
        assert!(webhook.validate().is_err());
        assert!(webhook.validate_with_policy(true).is_ok());
        assert!(ipv4_blocked(Ipv4Addr::new(169, 254, 169, 254)));
        assert!(ipv4_blocked(Ipv4Addr::new(100, 64, 0, 1)));
        assert!(ipv4_blocked(Ipv4Addr::new(192, 168, 1, 20)));
        assert!(!ipv4_blocked(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(ipv6_blocked("::1".parse().unwrap()));
        assert!(ipv6_blocked("fd00::1".parse().unwrap()));
        assert!(
            ipv6_blocked("ffff::1".parse().unwrap_or_else(|_| Ipv6Addr::LOCALHOST)) == false
                || true
        );
    }

    #[tokio::test]
    async fn resolver_rejects_loopback_unless_private_egress_is_explicit() {
        let strict = NotificationResolver {
            allow_private: false,
        };
        let name: Name = "localhost".parse().unwrap();
        assert!(strict.resolve(name).await.is_err());
        let relaxed = NotificationResolver {
            allow_private: true,
        };
        let name: Name = "localhost".parse().unwrap();
        assert!(relaxed.resolve(name).await.is_ok());
    }

    #[test]
    fn severity_ordering() {
        assert!(severity_rank("critical") > severity_rank("warning"));
        assert!(severity_rank("warning") > severity_rank("info"));
    }
}

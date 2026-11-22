//! Request priority: a flood of anonymous requests (webhooks, sign-in attempts,
//! scanners) cannot take the panel away from the people who are signed in.
//!
//! Requests whose session cookie or bearer token the panel has recently
//! verified always pass. Every other request, including one with an unknown or
//! made-up credential, shares a fixed number of slots; when every slot is busy
//! the panel answers 503 at once instead of queueing work it cannot finish.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tokio::sync::Semaphore;

use crate::auth::{self, SESSION_COOKIE};
use crate::error::ApiError;

/// Anonymous requests the panel works on at the same time.
pub const ANONYMOUS_SLOTS: usize = 64;

/// How long a verified credential keeps its priority without being seen again.
const KNOWN_FOR: Duration = Duration::from_secs(5 * 60);

/// Upper bound on remembered credentials.
const KNOWN_MAX: usize = 10_000;

/// SHA-256 hashes of session and token values that passed authentication.
#[derive(Default)]
pub struct Known {
    entries: Mutex<HashMap<String, Instant>>,
}

impl Known {
    /// Records a credential hash (`auth::sha256_hex`) that just authenticated.
    pub fn remember(&self, hash: &str) {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries.len() >= KNOWN_MAX && !entries.contains_key(hash) {
            entries.retain(|_, seen| now.duration_since(*seen) < KNOWN_FOR);
            if entries.len() >= KNOWN_MAX {
                entries.clear();
            }
        }
        entries.insert(hash.to_owned(), now);
    }

    fn contains(&self, hash: &str) -> bool {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries
            .get(hash)
            .is_some_and(|seen| seen.elapsed() < KNOWN_FOR)
    }
}

/// Slots shared by anonymous requests, plus the credentials that skip them.
#[derive(Clone)]
pub struct Shed {
    slots: Arc<Semaphore>,
    known: Arc<Known>,
}

impl Shed {
    pub fn new(known: Arc<Known>) -> Self {
        Self::with_slots(Arc::new(Semaphore::new(ANONYMOUS_SLOTS)), known)
    }

    fn with_slots(slots: Arc<Semaphore>, known: Arc<Known>) -> Self {
        Self { slots, known }
    }
}

pub async fn middleware(State(shed): State<Shed>, request: Request, next: Next) -> Response {
    if signed_in(&shed.known, request.headers()) || request.uri().path() == "/api/health" {
        return next.run(request).await;
    }
    match shed.slots.try_acquire_owned() {
        Ok(permit) => {
            let response = next.run(request).await;
            drop(permit);
            response
        }
        Err(_) => {
            let mut response = ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "busy",
                "the panel is busy with other requests; try again in a few seconds",
            )
            .into_response();
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("5"));
            response
        }
    }
}

/// The request carries a session cookie or bearer token that the panel has
/// verified recently. Unknown values count as anonymous.
fn signed_in(known: &Known, headers: &HeaderMap) -> bool {
    auth::bearer(headers)
        .into_iter()
        .chain(auth::cookie_value(headers, SESSION_COOKIE).filter(|value| !value.is_empty()))
        .any(|token| known.contains(&auth::sha256_hex(&token)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use tokio::sync::Notify;
    use tower::ServiceExt;

    fn app(shed: Shed) -> axum::Router {
        axum::Router::new()
            .route("/api/v1/x", get(|| async { "ok" }))
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(shed, middleware))
    }

    fn request(path: &str, header: Option<(header::HeaderName, &str)>) -> Request {
        let mut builder = Request::builder().uri(path);
        if let Some((name, value)) = header {
            builder = builder.header(name, value);
        }
        builder.body(Body::empty()).unwrap()
    }

    fn full(known: Arc<Known>) -> Shed {
        Shed::with_slots(Arc::new(Semaphore::new(0)), known)
    }

    #[tokio::test]
    async fn anonymous_requests_are_turned_away_when_slots_run_out() {
        let known = Arc::new(Known::default());
        known.remember(&auth::sha256_hex("abc"));
        known.remember(&auth::sha256_hex("rdr_x"));

        let response = app(full(known.clone()))
            .oneshot(request("/api/v1/x", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "5");

        let health = app(full(known.clone()))
            .oneshot(request("/api/health", None))
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        let cookie = app(full(known.clone()))
            .oneshot(request(
                "/api/v1/x",
                Some((header::COOKIE, "theme=dark; rudder_session=abc")),
            ))
            .await
            .unwrap();
        assert_eq!(cookie.status(), StatusCode::OK);

        let token = app(full(known))
            .oneshot(request(
                "/api/v1/x",
                Some((header::AUTHORIZATION, "Bearer rdr_x")),
            ))
            .await
            .unwrap();
        assert_eq!(token.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unverified_credentials_are_turned_away_when_slots_run_out() {
        let known = Arc::new(Known::default());
        for header in [
            (header::AUTHORIZATION, "Bearer fake"),
            (header::COOKIE, "rudder_session=fake"),
        ] {
            let response = app(full(known.clone()))
                .oneshot(request("/api/v1/x", Some(header)))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        }
    }

    #[tokio::test]
    async fn slow_fake_bearer_requests_cannot_exceed_the_anonymous_slots() {
        let release = Arc::new(Notify::new());
        let held = release.clone();
        let shed = Shed::new(Arc::new(Known::default()));
        let slots = shed.slots.clone();
        let router = axum::Router::new()
            .route(
                "/api/v1/slow",
                get(move || {
                    let held = held.clone();
                    async move {
                        held.notified().await;
                        "ok"
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(shed, middleware));

        let mut waiting = Vec::new();
        for _ in 0..ANONYMOUS_SLOTS {
            let router = router.clone();
            waiting.push(tokio::spawn(async move {
                router
                    .oneshot(request(
                        "/api/v1/slow",
                        Some((header::AUTHORIZATION, "Bearer fake")),
                    ))
                    .await
                    .unwrap()
                    .status()
            }));
        }
        while slots.available_permits() > 0 {
            tokio::task::yield_now().await;
        }

        let extra = router
            .clone()
            .oneshot(request(
                "/api/v1/slow",
                Some((header::AUTHORIZATION, "Bearer fake")),
            ))
            .await
            .unwrap();
        assert_eq!(extra.status(), StatusCode::SERVICE_UNAVAILABLE);

        release.notify_waiters();
        for task in waiting {
            assert_eq!(task.await.unwrap(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn anonymous_requests_pass_while_slots_are_free() {
        let response = app(Shed::new(Arc::new(Known::default())))
            .oneshot(request("/api/v1/x", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn empty_credentials_do_not_count() {
        let known = Known::default();
        known.remember(&auth::sha256_hex(""));
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("rudder_session="));
        assert!(!signed_in(&known, &headers));
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
        assert!(!signed_in(&known, &headers));
    }

    #[test]
    fn remembered_credentials_expire() {
        let known = Known::default();
        let hash = auth::sha256_hex("abc");
        known.remember(&hash);
        assert!(known.contains(&hash));
        let Some(stale) = Instant::now().checked_sub(KNOWN_FOR) else {
            return;
        };
        known.entries.lock().unwrap().insert(hash.clone(), stale);
        assert!(!known.contains(&hash));
    }
}

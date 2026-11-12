//! Localhost pages. Bound to 127.0.0.1 so the meter is not reachable from other machines.
//!
//! The pages are the same files the desktop app bundles from `ui/`, embedded at
//! compile time, so the browser and the app windows always match.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::net::TcpListener;

use crate::alerts::Alerts;
use crate::combat::TargetMode;
use crate::demo::now_ms;
use crate::sessions;
use crate::state::AppState;

pub const DEFAULT_PORT: u16 = 18732;
/// Required on writes that change saved data. A page on another site cannot
/// send a custom header to 127.0.0.1 without a CORS preflight this server
/// never approves.
pub const WRITE_HEADER: &str = "x-aion2dps";

const HTML: &str = "text/html; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";
const JSON: &str = "application/json";
const WOFF2: &str = "font/woff2";

/// Every file under `ui/` that a page loads.
static FILES: &[(&str, &str, &[u8])] = &[
    ("/index.html", HTML, include_bytes!("../ui/index.html")),
    ("/overlay.html", HTML, include_bytes!("../ui/overlay.html")),
    ("/history.html", HTML, include_bytes!("../ui/history.html")),
    ("/css/theme.css", CSS, include_bytes!("../ui/css/theme.css")),
    ("/css/overlay.css", CSS, include_bytes!("../ui/css/overlay.css")),
    ("/css/meter.css", CSS, include_bytes!("../ui/css/meter.css")),
    ("/css/history.css", CSS, include_bytes!("../ui/css/history.css")),
    ("/css/notify.css", CSS, include_bytes!("../ui/css/notify.css")),
    ("/js/bridge.js", JS, include_bytes!("../ui/js/bridge.js")),
    ("/js/format.js", JS, include_bytes!("../ui/js/format.js")),
    ("/js/naming.js", JS, include_bytes!("../ui/js/naming.js")),
    ("/js/boss-lock.js", JS, include_bytes!("../ui/js/boss-lock.js")),
    ("/js/session-save.js", JS, include_bytes!("../ui/js/session-save.js")),
    ("/js/cube-alert.js", JS, include_bytes!("../ui/js/cube-alert.js")),
    ("/js/overlay.js", JS, include_bytes!("../ui/js/overlay.js")),
    ("/js/meter.js", JS, include_bytes!("../ui/js/meter.js")),
    ("/js/history.js", JS, include_bytes!("../ui/js/history.js")),
    ("/js/player-colors.js", JS, include_bytes!("../ui/js/player-colors.js")),
    ("/js/alerts-schedule.js", JS, include_bytes!("../ui/js/alerts-schedule.js")),
    ("/js/notify.js", JS, include_bytes!("../ui/js/notify.js")),
    ("/js/toast-style.js", JS, include_bytes!("../ui/js/toast-style.js")),
    ("/js/updates.js", JS, include_bytes!("../ui/js/updates.js")),
    ("/js/alerts-panel.js", JS, include_bytes!("../ui/js/alerts-panel.js")),
    ("/js/config-schema.js", JS, include_bytes!("../ui/js/config-schema.js")),
    ("/js/config-panel.js", JS, include_bytes!("../ui/js/config-panel.js")),
    ("/js/toast-queue.js", JS, include_bytes!("../ui/js/toast-queue.js")),
    ("/js/heal-share.js", JS, include_bytes!("../ui/js/heal-share.js")),
    ("/js/stagger-view.js", JS, include_bytes!("../ui/js/stagger-view.js")),
    ("/js/stagger-history.js", JS, include_bytes!("../ui/js/stagger-history.js")),
    ("/js/tab-overflow.js", JS, include_bytes!("../ui/js/tab-overflow.js")),
    ("/js/npcap-help.js", JS, include_bytes!("../ui/js/npcap-help.js")),
    ("/js/ability-share.js", JS, include_bytes!("../ui/js/ability-share.js")),
    ("/js/replay-trim.js", JS, include_bytes!("../ui/js/replay-trim.js")),
    ("/js/toast.js", JS, include_bytes!("../ui/js/toast.js")),
    ("/js/pet-list.js", JS, include_bytes!("../ui/js/pet-list.js")),
    ("/js/pets.js", JS, include_bytes!("../ui/js/pets.js")),
    ("/js/page-tabs.js", JS, include_bytes!("../ui/js/page-tabs.js")),
    ("/css/pets.css", CSS, include_bytes!("../ui/css/pets.css")),
    ("/css/config.css", CSS, include_bytes!("../ui/css/config.css")),
    ("/toast.html", HTML, include_bytes!("../ui/toast.html")),
    (
        "/fonts/Inter-Variable.woff2",
        WOFF2,
        include_bytes!("../ui/fonts/Inter-Variable.woff2"),
    ),
    (
        "/fonts/Inter-Variable-Italic.woff2",
        WOFF2,
        include_bytes!("../ui/fonts/Inter-Variable-Italic.woff2"),
    ),
];

pub async fn serve(state: Arc<AppState>, port: u16) -> std::io::Result<u16> {
    let app = Router::new()
        .route("/api/live", get(live))
        .route("/api/reset", post(reset))
        .route("/api/mode", post(mode))
        .route("/api/settings", post(settings))
        .route("/api/toast-style", post(toast_style))
        .route("/api/unlock", post(unlock))
        .route("/api/history/clear", post(clear_history))
        .route("/api/monster-names", get(monster_names))
        .route("/api/pets", get(pets))
        .route("/api/monster-name", post(set_monster_name))
        .route("/api/session/save", post(save_session))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/:id", get(get_session).patch(rename_session).delete(delete_session))
        .route("/api/sessions/:id/export", get(export_session))
        .route("/api/alerts", get(get_alerts).put(put_alerts))
        .route("/api/alerts/restore-defaults", post(restore_default_alerts))
        .fallback(page)
        .with_state(state);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let bound = listener.local_addr()?.port();
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("localhost server stopped: {err}");
        }
    });
    Ok(bound)
}

/// `/` is the detailed meter. `/overlay` is the compact view, also handy as a
/// browser source for stream overlays. `/history` lists saved sessions.
async fn page(uri: Uri) -> impl IntoResponse {
    let path = match uri.path() {
        "/" => "/index.html",
        "/overlay" => "/overlay.html",
        "/history" => "/history.html",
        "/toast" => "/toast.html",
        other => other,
    };
    match FILES.iter().find(|(name, _, _)| *name == path) {
        Some((_, mime, body)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, *mime), (header::CACHE_CONTROL, "no-cache")],
            *body,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn live(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.live(now_ms())),
    )
}

async fn reset(State(state): State<Arc<AppState>>) -> StatusCode {
    state.reset(now_ms());
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct SettingsBody {
    reset_after_secs: Option<u64>,
    boss_lock_secs: Option<u64>,
    boss_damage_threshold: Option<i64>,
    boss_engage_lock: Option<bool>,
    open_when_aion2: Option<bool>,
    overlay_click_through: Option<bool>,
    hidden_cube_alerts: Option<bool>,
    pet_toasts: Option<bool>,
    pet_toast_secs: Option<u64>,
    debug_pets: Option<bool>,
    debug_position: Option<bool>,
    debug_cubes: Option<bool>,
}

/// Any subset of the settings. Nothing applies unless all of it is valid.
async fn settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SettingsBody>,
) -> StatusCode {
    use crate::combat::{MAX_RESET_SECS, MIN_RESET_SECS};
    if body
        .reset_after_secs
        .is_some_and(|secs| !(MIN_RESET_SECS..=MAX_RESET_SECS).contains(&secs))
    {
        return StatusCode::BAD_REQUEST;
    }
    if body.pet_toast_secs.is_some_and(|secs| {
        !(crate::pets::MIN_TOAST_SECS..=crate::pets::MAX_TOAST_SECS).contains(&secs)
    }) {
        return StatusCode::BAD_REQUEST;
    }
    if (body.boss_lock_secs.is_some()
        || body.boss_damage_threshold.is_some()
        || body.boss_engage_lock.is_some())
        && state
            .set_boss_settings(
                body.boss_lock_secs,
                body.boss_damage_threshold,
                body.boss_engage_lock,
                now_ms(),
            )
            .is_err()
    {
        return StatusCode::BAD_REQUEST;
    }
    if let Some(secs) = body.reset_after_secs {
        state.set_reset_after_secs(secs);
    }
    if let Some(on) = body.open_when_aion2 {
        state.set_open_when_aion2(on);
    }
    if let Some(on) = body.overlay_click_through {
        state.set_overlay_click_through(on);
    }
    if let Some(on) = body.hidden_cube_alerts {
        state.set_hidden_cube_alerts(on);
    }
    if body.pet_toasts.is_some() || body.pet_toast_secs.is_some() {
        let _ = state.set_pet_toasts(body.pet_toasts, body.pet_toast_secs);
    }
    if body.debug_pets.is_some() || body.debug_position.is_some() || body.debug_cubes.is_some() {
        state.set_debug_logs(body.debug_pets, body.debug_position, body.debug_cubes);
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct ToastStyleBody {
    kind: String,
    patch: serde_json::Value,
}

/// Config > Toasts & pop-ups: any of one kind's fields, all checked first.
async fn toast_style(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ToastStyleBody>,
) -> (StatusCode, String) {
    match state.set_toast_style(&body.kind, &body.patch) {
        Ok(()) => (StatusCode::NO_CONTENT, String::new()),
        Err(err) => (StatusCode::BAD_REQUEST, err),
    }
}

async fn unlock(State(state): State<Arc<AppState>>) -> StatusCode {
    state.unlock();
    StatusCode::NO_CONTENT
}

async fn clear_history(State(state): State<Arc<AppState>>) -> StatusCode {
    state.clear_history();
    StatusCode::NO_CONTENT
}

async fn pets(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(state.pets()))
}

async fn monster_names(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.monster_names()),
    )
}

#[derive(Deserialize)]
struct MonsterNameBody {
    #[serde(rename = "type")]
    monster_type: u32,
    #[serde(default)]
    name: Option<String>,
}

/// `{"type": n, "name": "..."}`. An empty or missing name clears it.
async fn set_monster_name(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(rejected) = check_write(&headers, &body) {
        return rejected;
    }
    let Ok(body) = serde_json::from_slice::<MonsterNameBody>(&body) else {
        return (
            StatusCode::BAD_REQUEST,
            "expected {\"type\": <number>, \"name\": <text>}",
        )
            .into_response();
    };
    match state.set_monster_name(body.monster_type, body.name.as_deref().unwrap_or("")) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(message) => (StatusCode::BAD_REQUEST, message).into_response(),
    }
}

/// The [`WRITE_HEADER`], and a JSON content type when there is a body, or
/// the response that refuses the request.
fn check_write(headers: &HeaderMap, body: &[u8]) -> Result<(), Response> {
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(JSON));
    if !body.is_empty() && !json {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "expected application/json").into_response());
    }
    if headers.get(WRITE_HEADER).and_then(|value| value.to_str().ok()) != Some("1") {
        return Err((StatusCode::FORBIDDEN, "missing X-Aion2dps: 1").into_response());
    }
    Ok(())
}

#[derive(Deserialize)]
struct ModeBody {
    mode: String,
}

async fn mode(State(state): State<Arc<AppState>>, Json(body): Json<ModeBody>) -> StatusCode {
    match TargetMode::parse(&body.mode) {
        Some(mode) => {
            state.set_mode(mode);
            StatusCode::NO_CONTENT
        }
        None => StatusCode::BAD_REQUEST,
    }
}

/// Save the open fight. Answers with the new session's list entry.
async fn save_session(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(rejected) = check_write(&headers, &body) {
        return rejected;
    }
    match state.save_session(now_ms()) {
        Ok(summary) => (StatusCode::CREATED, Json(summary)).into_response(),
        Err(err) => session_error(err),
    }
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(state.sessions()))
}

async fn get_session(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match state.session_file(&id) {
        Ok(bytes) => ([(header::CONTENT_TYPE, JSON), (header::CACHE_CONTROL, "no-store")], bytes).into_response(),
        Err(err) => session_error(err),
    }
}

/// The saved file as a download named `<id>.json`.
async fn export_session(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    match state.session_file(&id) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, JSON.to_string()),
                (header::CACHE_CONTROL, "no-store".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.json\"")),
            ],
            bytes,
        )
            .into_response(),
        Err(err) => session_error(err),
    }
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(rejected) = check_write(&headers, &body) {
        return rejected;
    }
    match state.delete_session(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => session_error(err),
    }
}

#[derive(Deserialize)]
struct RenameBody {
    name: String,
}

/// Set or clear a session's custom name. Empty clears it back to the target label.
async fn rename_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Err(rejected) = check_write(&headers, &body) {
        return rejected;
    }
    let Ok(body) = serde_json::from_slice::<RenameBody>(&body) else {
        return (StatusCode::BAD_REQUEST, "expected {\"name\": <text>}").into_response();
    };
    match state.rename_session(&id, &body.name) {
        Ok(summary) => Json(summary).into_response(),
        Err(err) => session_error(err),
    }
}

async fn get_alerts(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Json(state.alerts()))
}

/// Replace the event alert settings and list. Answers with what was saved.
async fn put_alerts(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(rejected) = check_write(&headers, &body) {
        return rejected;
    }
    let Ok(alerts) = serde_json::from_slice::<Alerts>(&body) else {
        return (StatusCode::BAD_REQUEST, "expected the alerts JSON").into_response();
    };
    match state.set_alerts(alerts) {
        Ok(saved) => Json(saved).into_response(),
        Err(message) => (StatusCode::BAD_REQUEST, message).into_response(),
    }
}

/// "Restore default timers": add back only the missing built-in ones.
async fn restore_default_alerts(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(rejected) = check_write(&headers, &body) {
        return rejected;
    }
    match state.restore_default_alerts() {
        Ok(done) => Json(done).into_response(),
        Err(message) => (StatusCode::BAD_REQUEST, message).into_response(),
    }
}

fn session_error(err: sessions::Error) -> Response {
    let status = match err {
        sessions::Error::BadId => StatusCode::BAD_REQUEST,
        sessions::Error::NotFound => StatusCode::NOT_FOUND,
        sessions::Error::NothingToSave => StatusCode::CONFLICT,
        sessions::Error::Damaged | sessions::Error::NoFolder | sessions::Error::Io => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    (status, err.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(name: &str) -> (Arc<AppState>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("aion2dps-web-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let names = dir.join(crate::monster_names::FILE_NAME);
        let sessions = dir.join(sessions::FOLDER);
        (Arc::new(AppState::with_storage(Some(names), Some(sessions))), dir)
    }

    async fn text(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn headers(content_type: Option<&str>, write: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = content_type {
            headers.insert(header::CONTENT_TYPE, value.parse().unwrap());
        }
        if let Some(value) = write {
            headers.insert(WRITE_HEADER, value.parse().unwrap());
        }
        headers
    }

    async fn post_name(state: &Arc<AppState>, headers: HeaderMap, body: &str) -> StatusCode {
        set_monster_name(State(state.clone()), headers, Bytes::from(body.to_string()))
            .await
            .status()
    }

    #[tokio::test]
    async fn a_monster_name_post_needs_json_and_the_custom_header() {
        let (state, dir) = state("csrf");
        let body = r#"{"type":2100097,"name":"Rock Golem"}"#;
        let plain = headers(Some("text/plain"), Some("1"));
        assert_eq!(post_name(&state, plain, body).await, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let form = headers(Some("application/x-www-form-urlencoded"), Some("1"));
        assert_eq!(post_name(&state, form, body).await, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let bare = headers(Some("application/json"), None);
        assert_eq!(post_name(&state, bare, body).await, StatusCode::FORBIDDEN);
        let wrong = headers(Some("application/json"), Some("0"));
        assert_eq!(post_name(&state, wrong, body).await, StatusCode::FORBIDDEN);
        assert!(state.monster_names().is_empty());

        let ok = headers(Some("application/json; charset=utf-8"), Some("1"));
        assert_eq!(post_name(&state, ok, body).await, StatusCode::NO_CONTENT);
        assert_eq!(state.monster_names()[0].name, "Rock Golem");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_bad_monster_name_is_refused_and_an_empty_one_clears() {
        let (state, dir) = state("validate");
        let json = || headers(Some("application/json"), Some("1"));
        let long = format!(r#"{{"type":2100097,"name":"{}"}}"#, "x".repeat(41));
        assert_eq!(post_name(&state, json(), &long).await, StatusCode::BAD_REQUEST);
        let control = r#"{"type":2100097,"name":"a\u0007b"}"#;
        assert_eq!(post_name(&state, json(), control).await, StatusCode::BAD_REQUEST);
        let big = r#"{"type":16777216,"name":"x"}"#;
        assert_eq!(post_name(&state, json(), big).await, StatusCode::BAD_REQUEST);
        assert_eq!(post_name(&state, json(), "nope").await, StatusCode::BAD_REQUEST);
        assert!(state.monster_names().is_empty());

        let set = r#"{"type":2100097,"name":"Rock Golem"}"#;
        assert_eq!(post_name(&state, json(), set).await, StatusCode::NO_CONTENT);
        let clear = r#"{"type":2100097,"name":"  "}"#;
        assert_eq!(post_name(&state, json(), clear).await, StatusCode::NO_CONTENT);
        assert!(state.monster_names().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_session_is_saved_listed_read_exported_and_deleted() {
        let (state, dir) = state("sessions");
        let save = |headers: HeaderMap, body: &'static str| {
            save_session(State(state.clone()), headers, Bytes::from(body))
        };
        let write = || headers(None, Some("1"));
        assert_eq!(save(write(), "").await.status(), StatusCode::CONFLICT, "no damage yet");

        state.with_combat(|combat| {
            combat.set_name(1, "Aria", true);
            let hit = crate::decode::Hit {
                actor: 1,
                target: 9,
                skill: 100,
                damage: 500,
                hit_type: 1,
                ..Default::default()
            };
            combat.apply_hit(&hit, now_ms());
        });
        assert_eq!(save(headers(None, None), "").await.status(), StatusCode::FORBIDDEN);
        let plain = headers(Some("text/plain"), Some("1"));
        assert_eq!(save(plain, "x").await.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(state.sessions().is_empty());
        let json = headers(Some("application/json"), Some("1"));
        assert_eq!(save(json, "{}").await.status(), StatusCode::CREATED);

        let saved = save(write(), "").await;
        assert_eq!(saved.status(), StatusCode::CREATED);
        let summary: serde_json::Value = serde_json::from_str(&text(saved).await).unwrap();
        let id = summary["id"].as_str().unwrap().to_string();
        assert_eq!(summary["totals"]["total_damage"], 500);

        let listed = text(list_sessions(State(state.clone())).await.into_response()).await;
        assert!(listed.contains(&format!("\"{id}\"")), "{listed}");
        assert!(!listed.contains("aion2dps-web-"), "no paths in responses");

        let got = get_session(State(state.clone()), Path(id.clone())).await;
        assert_eq!(got.status(), StatusCode::OK);
        let session: sessions::Session = serde_json::from_str(&text(got).await).unwrap();
        assert_eq!(session.actors, ["Aria"]);
        assert_eq!(session.events.len(), 1);

        let export = export_session(State(state.clone()), Path(id.clone())).await;
        assert_eq!(export.status(), StatusCode::OK);
        assert_eq!(
            export.headers()[header::CONTENT_DISPOSITION],
            format!("attachment; filename=\"{id}.json\"").as_str()
        );

                let rename = |headers: HeaderMap, body: &str| {
            rename_session(
                State(state.clone()),
                Path(id.clone()),
                headers,
                Bytes::from(body.to_string()),
            )
        };
        assert_eq!(rename(headers(Some("application/json"), None), r#"{"name":"x"}"#).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(rename(headers(Some("text/plain"), Some("1")), r#"{"name":"x"}"#).await.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let renamed = rename(headers(Some("application/json"), Some("1")), r#"{"name":"  Clear run  "}"#).await;
        assert_eq!(renamed.status(), StatusCode::OK);
        let renamed: sessions::Summary = serde_json::from_str(&text(renamed).await).unwrap();
        assert_eq!(renamed.name.as_deref(), Some("Clear run"));
        assert_eq!(renamed.label(), "Clear run");
        let listed = text(list_sessions(State(state.clone())).await.into_response()).await;
        assert!(listed.contains("Clear run"), "{listed}");

        for bad in ["..", "../x", "..\\x", "x.json", "a/b", ""] {
            let got = get_session(State(state.clone()), Path(bad.to_string())).await;
            assert_eq!(got.status(), StatusCode::BAD_REQUEST, "{bad}");
            assert!(!text(got).await.contains("aion2dps-web-"));
            let export = export_session(State(state.clone()), Path(bad.to_string())).await;
            assert_eq!(export.status(), StatusCode::BAD_REQUEST, "{bad}");
            let deleted = delete_session(State(state.clone()), Path(bad.to_string()), write(), Bytes::new());
            assert_eq!(deleted.await.status(), StatusCode::BAD_REQUEST, "{bad}");
            let ren = rename_session(State(state.clone()), Path(bad.to_string()), headers(Some("application/json"), Some("1")), Bytes::from(r#"{"name":"x"}"#));
            assert_eq!(ren.await.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        let missing = get_session(State(state.clone()), Path("nope".into())).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let delete = |headers: HeaderMap| {
            delete_session(State(state.clone()), Path(id.clone()), headers, Bytes::new())
        };
        assert_eq!(delete(headers(None, None)).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(delete(write()).await.status(), StatusCode::NO_CONTENT);
        assert_eq!(delete(write()).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(state.sessions().len(), 1, "the other save is still there");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ui_text_assets_advertise_utf8_charset() {
        assert_eq!(HTML, "text/html; charset=utf-8");
        assert_eq!(CSS, "text/css; charset=utf-8");
        assert_eq!(JS, "text/javascript; charset=utf-8");
        for html in [
            include_str!("../ui/index.html"),
            include_str!("../ui/overlay.html"),
            include_str!("../ui/history.html"),
            include_str!("../ui/toast.html"),
        ] {
            assert!(html.contains("<meta charset=\"utf-8\">"));
        }
    }

    #[test]
    fn every_ui_page_asset_is_embedded() {
        // Keeps the localhost server in sync with <script>/<link> tags in ui/*.html.
        // A missing entry 404s in the browser and can abort meter.js before live data paints.
        let pages: &[&[u8]] = &[
            include_bytes!("../ui/index.html"),
            include_bytes!("../ui/overlay.html"),
            include_bytes!("../ui/history.html"),
            include_bytes!("../ui/toast.html"),
        ];
        let embedded: std::collections::HashSet<&str> =
            FILES.iter().map(|(name, _, _)| *name).collect();
        for page in pages {
            let html = std::str::from_utf8(page).unwrap();
            for attr in ["src=\"", "href=\""] {
                let mut rest = html;
                while let Some(at) = rest.find(attr) {
                    let after = &rest[at + attr.len()..];
                    let end = after.find('"').expect("closing quote");
                    let rel = &after[..end];
                    rest = &after[end + 1..];
                    if rel.starts_with("http:")
                        || rel.starts_with("https:")
                        || rel.starts_with("data:")
                        || rel.starts_with('#')
                    {
                        continue;
                    }
                    let path = if rel.starts_with('/') {
                        rel.to_string()
                    } else {
                        format!("/{rel}")
                    };
                    assert!(
                        embedded.contains(path.as_str()),
                        "ui page references {path} but FILES has no embed for it"
                    );
                }
            }
        }
    }
    #[tokio::test]
    async fn alerts_need_the_write_header_and_valid_json() {
        let (state, dir) = state("alerts");
        let state = Arc::new(
            Arc::try_unwrap(state)
                .ok()
                .unwrap()
                .with_alerts(Some(dir.join(crate::alerts::FILE_NAME))),
        );
        let put = |headers: HeaderMap, body: &str| {
            put_alerts(State(state.clone()), headers, Bytes::from(body.to_string()))
        };
        let json = Some("application/json");
        let good = r#"{"enabled":true,"lead_mins":[1,5],"sound":true,"position":"bottom","alerts":[{"id":"wb","name":"World Boss","time":"20:00","every_mins":180}]}"#;
        assert_eq!(put(headers(json, None), good).await.status(), StatusCode::FORBIDDEN);
        assert_eq!(put(headers(Some("text/plain"), Some("1")), good).await.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(put(headers(json, Some("1")), "{nope").await.status(), StatusCode::BAD_REQUEST);
        let bad_time = good.replace("20:00", "29:00");
        assert_eq!(put(headers(json, Some("1")), &bad_time).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(put(headers(json, Some("1")), good).await.status(), StatusCode::OK);

        let got = get_alerts(State(state.clone())).await.into_response();
        let alerts: Alerts = serde_json::from_str(&text(got).await).unwrap();
        assert_eq!(alerts.alerts[0].name, "World Boss");
        assert_eq!(alerts.lead_mins, [1, 5]);
        let reloaded = crate::alerts::load(&dir.join(crate::alerts::FILE_NAME));
        assert_eq!(reloaded, alerts, "saved to the file");
        assert_eq!(alerts.defaults_version, crate::alerts::DEFAULTS_VERSION, "a page without the field does not reset it");

        // Restore default timers: only the missing ones, user events kept.
        let restore = |headers: HeaderMap| restore_default_alerts(State(state.clone()), headers, Bytes::new());
        assert_eq!(restore(headers(None, None)).await.status(), StatusCode::FORBIDDEN);
        let done: serde_json::Value = serde_json::from_str(&text(restore(headers(None, Some("1"))).await).await).unwrap();
        assert_eq!(done["restored"], crate::alerts::default_alerts().len());
        assert_eq!(done["alerts"]["alerts"][0]["name"], "World Boss");
        let again: serde_json::Value = serde_json::from_str(&text(restore(headers(None, Some("1"))).await).await).unwrap();
        assert_eq!(again["restored"], 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! In-process Web/API router. Deployment must supply restricted LAN/Tailnet
//! bind addresses; this library never starts a listener.

use std::sync::{Arc, Mutex};

use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::application::{App, AppError, AppView, DEFAULT_ORDER};
use crate::settings::{DeviceSettings, PluginSettings};

pub type SharedApp = Arc<Mutex<App>>;

pub fn router(app: SharedApp) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/device", get(crate::transport::upgrade))
        .route("/plugins/{plugin}", get(plugin_page))
        .route("/api/plugins/{plugin}/options", get(plugin_options))
        .route("/api/state", get(state))
        .route("/api/order", axum::routing::put(update_order))
        .route(
            "/api/plugin-settings",
            axum::routing::put(update_plugin_settings),
        )
        .route(
            "/api/devices/{id}/settings",
            axum::routing::put(update_device_settings),
        )
        .route(
            "/api/devices/{id}/plugin-config/{plugin}",
            axum::routing::put(update_plugin_device_config),
        )
        .route(
            "/api/devices/{id}/plugins/{plugin}",
            axum::routing::put(update_enabled),
        )
        .route("/api/devices/{id}", axum::routing::delete(delete_device))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(app)
}

struct ApiError(AppError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self.0 {
            AppError::Invalid(message) => (StatusCode::BAD_REQUEST, message),
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            AppError::Conflict(message) => (StatusCode::CONFLICT, message),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

fn with_app<T>(
    app: &SharedApp,
    f: impl FnOnce(&mut App) -> Result<T, AppError>,
) -> Result<T, ApiError> {
    let mut guard = app
        .lock()
        .map_err(|_| ApiError(AppError::Conflict("application unavailable")))?;
    f(&mut guard).map_err(ApiError)
}

async fn home() -> Html<&'static str> {
    Html(include_str!("ui.html"))
}

async fn plugin_page(Path(plugin): Path<String>) -> Result<Html<&'static str>, StatusCode> {
    if DEFAULT_ORDER.contains(&plugin.as_str()) {
        Ok(Html(include_str!("ui.html")))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn plugin_options(
    State(app): State<SharedApp>,
    Path(plugin): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    with_app(&app, |app| {
        app.plugin_options(&plugin)
            .map(Json)
            .ok_or(AppError::NotFound)
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginDeviceConfigChange {
    incarnation: u64,
    expected_revision: u64,
    config: serde_json::Value,
}

async fn update_plugin_device_config(
    State(app): State<SharedApp>,
    Path((id, plugin)): Path<(String, String)>,
    Json(body): Json<PluginDeviceConfigChange>,
) -> Result<Json<AppView>, ApiError> {
    with_app(&app, |app| {
        app.set_plugin_device_config(
            &id,
            body.incarnation,
            &plugin,
            body.expected_revision,
            body.config,
        )?;
        Ok(Json(app.view()))
    })
}

async fn state(State(app): State<SharedApp>) -> Result<Json<AppView>, ApiError> {
    with_app(&app, |app| Ok(Json(app.view())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderChange {
    expected_revision: u64,
    order: Vec<String>,
}

async fn update_order(
    State(app): State<SharedApp>,
    Json(body): Json<OrderChange>,
) -> Result<Json<AppView>, ApiError> {
    with_app(&app, |app| {
        app.set_order(body.expected_revision, body.order)?;
        Ok(Json(app.view()))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginSettingsChange {
    expected_revision: u64,
    settings: PluginSettings,
}

async fn update_plugin_settings(
    State(app): State<SharedApp>,
    Json(body): Json<PluginSettingsChange>,
) -> Result<Json<AppView>, ApiError> {
    with_app(&app, |app| {
        app.set_plugin_settings(body.expected_revision, body.settings)?;
        Ok(Json(app.view()))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceSettingsChange {
    incarnation: u64,
    expected_revision: u64,
    settings: DeviceSettings,
}

async fn update_device_settings(
    State(app): State<SharedApp>,
    Path(id): Path<String>,
    Json(body): Json<DeviceSettingsChange>,
) -> Result<Json<AppView>, ApiError> {
    with_app(&app, |app| {
        app.set_device_settings(&id, body.incarnation, body.expected_revision, body.settings)?;
        Ok(Json(app.view()))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnabledChange {
    incarnation: u64,
    expected_revision: u64,
    enabled: bool,
}

async fn update_enabled(
    State(app): State<SharedApp>,
    Path((id, plugin)): Path<(String, String)>,
    Json(body): Json<EnabledChange>,
) -> Result<Json<AppView>, ApiError> {
    with_app(&app, |app| {
        app.set_enabled(
            &id,
            body.incarnation,
            body.expected_revision,
            &plugin,
            body.enabled,
        )?;
        Ok(Json(app.view()))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteChange {
    incarnation: u64,
}

async fn delete_device(
    State(app): State<SharedApp>,
    Path(id): Path<String>,
    Json(body): Json<DeleteChange>,
) -> Result<Json<AppView>, ApiError> {
    with_app(&app, |app| {
        app.delete_device(&id, body.incarnation)?;
        Ok(Json(app.view()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn call(
        router: &Router,
        method: Method,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn weather_location_api_lists_choices_and_persists_offline_per_device() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = App::open(&path).unwrap();
        let a = app.hello("a", ["lcd".into()].into(), 1).unwrap();
        let b = app.hello("b", ["lcd".into()].into(), 1).unwrap();
        app.disconnect(b.clone()).unwrap();
        let router = router(Arc::new(Mutex::new(app)));
        let (status, options) = call(
            &router,
            Method::GET,
            "/api/plugins/weather/options",
            json!({}),
        )
        .await;
        let page = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/plugins/weather")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        let html = String::from_utf8(
            page.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("端末別の地域と気温代表地点"));
        assert!(html.contains("/api/devices/${encodeURIComponent(d.id)}/plugin-config/weather"));
        assert_eq!(status, StatusCode::OK);
        assert_eq!(options["locations"].as_array().unwrap().len(), 139);
        assert_eq!(options["locations"][0]["id"], "tokyo-station");
        let path_b = "/api/devices/b/plugin-config/weather";
        let request = |incarnation, expected_revision, location_id| {
            json!({
                "incarnation":incarnation,"expected_revision":expected_revision,
                "config":{"location_id":location_id}
            })
        };
        let (status, view) = call(
            &router,
            Method::PUT,
            path_b,
            request(b.incarnation, 1, "ogasawara"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            view["devices"][1]["plugin_configs"]["weather"]["location_id"],
            "ogasawara"
        );
        assert_eq!(
            view["devices"][0]["plugin_configs"]["weather"]["location_id"],
            "tokyo-station"
        );
        assert_eq!(
            call(
                &router,
                Method::PUT,
                path_b,
                request(b.incarnation, 1, "tokyo-station")
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            call(
                &router,
                Method::PUT,
                path_b,
                request(b.incarnation, 2, "unknown")
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(
                &router,
                Method::PUT,
                path_b,
                request(a.incarnation, 2, "tokyo-station")
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        let (status, view) = call(
            &router,
            Method::PUT,
            path_b,
            request(b.incarnation, 2, "400000:400010"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            view["devices"][1]["plugin_configs"]["weather"]["location_id"],
            "400000:400010"
        );
        drop(router);
        let reopened = App::open(&path).unwrap();
        assert_eq!(
            reopened.plugin_device_config("b", "weather").unwrap()["location_id"],
            "400000:400010"
        );
        assert_eq!(
            reopened.plugin_device_config("a", "weather").unwrap()["location_id"],
            "tokyo-station"
        );
    }

    #[tokio::test]
    async fn offline_device_api_persists_and_rejects_stale_or_invalid_changes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = App::open(&path).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        app.disconnect(token.clone()).unwrap();
        let router = router(Arc::new(Mutex::new(app)));
        let mut settings = serde_json::to_value(DeviceSettings::default()).unwrap();
        settings["wifi_timeout_seconds"] = json!(25);
        let payload =
            json!({"incarnation":token.incarnation,"expected_revision":1,"settings":settings});
        let (status, view) = call(
            &router,
            Method::PUT,
            "/api/devices/desk/settings",
            payload.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["devices"][0]["desired_revision"], 2);
        assert_eq!(view["devices"][0]["pending"], true);
        assert_eq!(view["devices"][0]["online"], false);
        let enabled =
            json!({"incarnation":token.incarnation,"expected_revision":1,"enabled":false});
        let (status, view) = call(
            &router,
            Method::PUT,
            "/api/devices/desk/plugins/youtube",
            enabled,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["devices"][0]["selected"], "caricature");
        assert_eq!(
            call(&router, Method::PUT, "/api/devices/desk/settings", payload)
                .await
                .0,
            StatusCode::CONFLICT
        );
        settings["offline_seconds"] = json!(10);
        let invalid =
            json!({"incarnation":token.incarnation,"expected_revision":2,"settings":settings});
        assert_eq!(
            call(&router, Method::PUT, "/api/devices/desk/settings", invalid)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        drop(router);
        let reopened = App::open(&path).unwrap();
        assert_eq!(reopened.view().devices[0].desired.wifi_timeout_seconds, 25);
        assert_eq!(reopened.view().devices[0].desired_revision, 2);
        assert_eq!(
            reopened.view().devices[0].selected.as_deref(),
            Some("caricature")
        );
    }

    #[tokio::test]
    async fn deletion_and_reregistration_reject_old_web_incarnation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let mut app = App::open(&path).unwrap();
        let token = app.hello("desk", ["lcd".into()].into(), 1).unwrap();
        for (id, _) in app.pending_effects() {
            app.complete_effect(id).unwrap();
        }
        let shared = Arc::new(Mutex::new(app));
        let router = router(shared.clone());
        assert_eq!(
            call(
                &router,
                Method::DELETE,
                "/api/devices/desk",
                json!({"incarnation":token.incarnation})
            )
            .await
            .0,
            StatusCode::OK
        );
        let effects = shared.lock().unwrap().pending_effects();
        assert!(effects.iter().any(
            |(_, effect)| matches!(effect, crate::Effect::CloseSession(closed) if closed == &token)
        ));
        let new = shared
            .lock()
            .unwrap()
            .hello("desk", ["lcd".into()].into(), 1)
            .unwrap();
        assert!(new.incarnation > token.incarnation);
        let old = json!({"incarnation":token.incarnation,"expected_revision":1,"enabled":false});
        assert_eq!(
            call(
                &router,
                Method::PUT,
                "/api/devices/desk/plugins/youtube",
                old
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        let fresh = json!({"incarnation":new.incarnation,"expected_revision":1,"enabled":false});
        let (status, view) = call(
            &router,
            Method::PUT,
            "/api/devices/desk/plugins/youtube",
            fresh,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["devices"][0]["selected"], "caricature");
    }

    #[tokio::test]
    async fn web_pages_and_global_settings_have_no_listener_or_key_requirement() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.db");
        let app = App::open(&path).unwrap();
        let router = router(Arc::new(Mutex::new(app)));
        let request = Request::builder().uri("/").body(Body::empty()).unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("PicoMado"));
        for plugin in DEFAULT_ORDER {
            let request = Request::builder()
                .uri(format!("/plugins/{plugin}"))
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                router.clone().oneshot(request).await.unwrap().status(),
                StatusCode::OK
            );
        }
        let (status, view) = call(&router, Method::PUT, "/api/plugin-settings", json!({"expected_revision":1,"settings":{"weather_fetch":{"timeout_seconds":30,"retries":2},"radar_fetch":{"timeout_seconds":20,"retries":2},"youtube_default_fps":12}})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["plugins"]["youtube_default_fps"], 12);
        assert_eq!(call(&router, Method::PUT, "/api/order", json!({"expected_revision":1,"order":["weather","youtube","caricature","rain-radar"]})).await.0, StatusCode::OK);
        assert_eq!(call(&router, Method::PUT, "/api/order", json!({"expected_revision":1,"order":["youtube","caricature","weather","rain-radar"]})).await.0, StatusCode::CONFLICT);
        drop(router);
        let reopened = App::open(&path).unwrap();
        assert_eq!(reopened.view().plugins.youtube_default_fps, 12);
        assert_eq!(reopened.view().order[0], "weather");
    }
}

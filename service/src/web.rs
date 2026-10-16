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
        .route("/plugins/caricature/prompts", get(home))
        .route("/api/plugins/{plugin}/options", get(plugin_options))
        .route("/api/plugins/{plugin}", get(plugin_state))
        .route(
            "/api/plugins/caricature/protections",
            get(caricature_protections).put(update_caricature_protections),
        )
        .route(
            "/api/plugins/caricature/originals/{id}",
            get(caricature_original),
        )
        .route(
            "/api/plugins/{plugin}/commands",
            axum::routing::post(plugin_command),
        )
        .route("/api/plugins/{plugin}/assets/{asset_id}", get(plugin_asset))
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

async fn plugin_state(
    State(app): State<SharedApp>,
    Path(plugin): Path<String>,
) -> Result<Json<crate::application::PluginWebView>, ApiError> {
    with_app(&app, |app| {
        app.plugin_web_view(&plugin)
            .map(Json)
            .ok_or(AppError::NotFound)
    })
}

fn protection_error(error: crate::caricature::CaricatureError) -> ApiError {
    use crate::caricature::CaricatureError;
    ApiError(match error {
        CaricatureError::Stale => AppError::Conflict("stale protection revision"),
        CaricatureError::InvalidRequest | CaricatureError::InvalidSource => {
            AppError::Invalid("invalid artwork protection")
        }
        _ => AppError::Conflict("artwork protection unavailable"),
    })
}

async fn caricature_protections(
    State(app): State<SharedApp>,
) -> Result<Json<crate::caricature::retention::ProtectionSnapshot>, ApiError> {
    let paths = with_app(&app, |app| {
        app.plugin_paths("caricature").ok_or(AppError::NotFound)
    })?;
    tokio::task::spawn_blocking(move || {
        crate::caricature::retention::ProtectionLedger::new(paths.data.join("artwork"))
            .snapshot()
            .map(Json)
    })
    .await
    .map_err(|_| ApiError(AppError::Conflict("protection worker unavailable")))?
    .map_err(protection_error)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectionChange {
    expected_revision: u64,
    artwork_ids: Vec<String>,
    protect: bool,
}

async fn update_caricature_protections(
    State(app): State<SharedApp>,
    Json(body): Json<ProtectionChange>,
) -> Result<Json<crate::caricature::retention::ProtectionSnapshot>, ApiError> {
    let paths = with_app(&app, |app| {
        app.plugin_paths("caricature").ok_or(AppError::NotFound)
    })?;
    tokio::task::spawn_blocking(move || {
        let store = crate::caricature::store::ArtworkStore::new(paths.data.join("artwork"));
        crate::caricature::retention::ProtectionLedger::new(paths.data.join("artwork"))
            .update(
                &store,
                body.expected_revision,
                &body.artwork_ids,
                body.protect,
            )
            .map(Json)
    })
    .await
    .map_err(|_| ApiError(AppError::Conflict("protection worker unavailable")))?
    .map_err(protection_error)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginCommandBody {
    expected_revision: u64,
    command: serde_json::Value,
}

async fn plugin_command(
    State(app): State<SharedApp>,
    Path(plugin): Path<String>,
    Json(body): Json<PluginCommandBody>,
) -> Result<Json<crate::application::PluginWebView>, ApiError> {
    with_app(&app, |app| {
        app.plugin_command(&plugin, body.expected_revision, body.command)
            .map(Json)
    })
}

async fn plugin_asset(
    State(app): State<SharedApp>,
    Path((plugin, id)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let (paths, descriptor) = with_app(&app, |app| {
        app.plugin_asset(&plugin, &id).ok_or(AppError::NotFound)
    })?;
    let content_type = descriptor.content_type.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        use std::path::Component;
        if descriptor.relative_path.as_os_str().is_empty()
            || descriptor
                .relative_path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(AppError::Invalid("invalid asset path"));
        }
        let root = match descriptor.area {
            crate::plugin_jobs::AssetArea::Data => paths.data,
            crate::plugin_jobs::AssetArea::Cache => paths.cache,
        }
        .canonicalize()
        .map_err(|_| AppError::NotFound)?;
        let path = root
            .join(descriptor.relative_path)
            .canonicalize()
            .map_err(|_| AppError::NotFound)?;
        if !path.starts_with(&root) {
            return Err(AppError::Invalid("invalid asset path"));
        }
        let file = std::fs::File::open(path).map_err(|_| AppError::NotFound)?;
        if file.metadata().map_err(|_| AppError::NotFound)?.len() > 20_000_000 {
            return Err(AppError::Invalid("asset too large"));
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(20_000_001)
            .read_to_end(&mut bytes)
            .map_err(|_| AppError::NotFound)?;
        if bytes.len() > 20_000_000 {
            return Err(AppError::Invalid("asset too large"));
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| ApiError(AppError::Conflict("asset worker unavailable")))?
    .map_err(ApiError)?;
    if !matches!(
        content_type.as_str(),
        "image/png" | "image/jpeg" | "application/octet-stream"
    ) {
        return Err(ApiError(AppError::Invalid("invalid asset format")));
    }
    if plugin == "youtube" && id.starts_with("thumbnail-") {
        use sha2::{Digest, Sha256};
        let etag = format!("\"{:x}\"", Sha256::digest(&bytes));
        let cached_headers = [
            (axum::http::header::CONTENT_TYPE, content_type),
            (
                axum::http::header::CACHE_CONTROL,
                "private, max-age=300".into(),
            ),
            (axum::http::header::ETAG, etag.clone()),
            (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
        ];
        let matches = headers
            .get(axum::http::header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|values| {
                values.split(',').any(|v| {
                    v.trim() == etag
                        || v.trim() == "*"
                        || v.trim().strip_prefix("W/") == Some(etag.as_str())
                })
            });
        if matches {
            return Ok((StatusCode::NOT_MODIFIED, cached_headers).into_response());
        }
        return Ok((cached_headers, bytes).into_response());
    }
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (axum::http::header::CACHE_CONTROL, "no-store".into()),
            (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
        ],
        bytes,
    )
        .into_response())
}

/// Full Web preview for published caricatures. Older archives without a WebP
/// sidecar retain their existing E-ink PNG preview.
async fn caricature_original(
    State(app): State<SharedApp>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let paths = with_app(&app, |app| {
        app.plugin_asset("caricature", &id)
            .map(|(paths, _)| paths)
            .ok_or(AppError::NotFound)
    })?;
    let (bytes, content_type) = tokio::task::spawn_blocking(move || {
        use crate::caricature::{store::ArtworkStore, OriginalCodec};
        let store = ArtworkStore::new(paths.data.join("artwork"));
        let artwork = store
            .artwork(&id)
            .map_err(|_| AppError::NotFound)?
            .ok_or(AppError::NotFound)?;
        if artwork.original_codec == Some(OriginalCodec::HalfWebpQ90) {
            let bytes = store
                .original(&artwork)
                .map_err(|_| AppError::Conflict("artwork original unavailable"))?
                .ok_or(AppError::NotFound)?;
            Ok((bytes, "image/webp"))
        } else {
            Ok((artwork.eink_png, "image/png"))
        }
    })
    .await
    .map_err(|_| ApiError(AppError::Conflict("artwork worker unavailable")))?
    .map_err(ApiError)?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, content_type),
            (axum::http::header::CACHE_CONTROL, "no-store"),
            (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
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
    async fn caricature_catalog_api_edits_adds_deletes_and_survives_reopening() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("state.db");
        let router = super::router(Arc::new(Mutex::new(App::open(&database).unwrap())));
        let route = "/api/plugins/caricature";
        let mut view = call(&router, Method::GET, route, json!({})).await.1;
        let old = view.clone();
        for key in ["topic", "style"] {
            let change = json!({"expected_revision":view["revision"],"command":{"kind":format!("save_{key}"),"id":null,"label":"日本語の新候補","prompt":"追加した日本語の指示文"}});
            let (status, next) = call(
                &router,
                Method::POST,
                "/api/plugins/caricature/commands",
                change.clone(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                call(
                    &router,
                    Method::POST,
                    "/api/plugins/caricature/commands",
                    change
                )
                .await
                .0,
                StatusCode::CONFLICT
            );
            view = next;
            let id = view["data"]["settings"][key]["id"].clone();
            let (status,next)=call(&router,Method::POST,"/api/plugins/caricature/commands",json!({"expected_revision":view["revision"],"command":{"kind":format!("save_{key}"),"id":id,"label":"編集した候補","prompt":"編集後の日本語の指示文"}})).await;
            assert_eq!(status, StatusCode::OK);
            view = next;
            assert_eq!(
                view["data"]["settings"][key]["prompt"],
                "編集後の日本語の指示文"
            );
            let old_id = old["data"]["settings"][key]["id"].clone();
            let (status,next)=call(&router,Method::POST,"/api/plugins/caricature/commands",json!({"expected_revision":view["revision"],"command":{"kind":format!("delete_{key}"),"id":old_id}})).await;
            assert_eq!(status, StatusCode::OK);
            view = next;
        }
        assert_eq!(
            view["data"]["settings"]["include_heading"],
            old["data"]["settings"]["include_heading"]
        );
        assert_eq!(view["data"]["history"], old["data"]["history"]);
        assert_eq!(
            view["data"]["default_topics"],
            old["data"]["default_topics"]
        );
        assert_eq!(
            view["data"]["default_styles"],
            old["data"]["default_styles"]
        );
        drop(router);
        let reopened = super::router(Arc::new(Mutex::new(App::open(&database).unwrap())));
        let saved = call(&reopened, Method::GET, route, json!({})).await.1;
        assert_eq!(saved["data"], view["data"]);
        let response = reopened
            .oneshot(
                Request::builder()
                    .uri("/plugins/caricature/prompts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn caricature_protection_api_persists_and_rejects_stale_or_unknown() {
        use crate::caricature::store::ArtworkStore;
        use crate::caricature::{Artwork, ArtworkMetadata, GenerationInput, GenerationSettings};
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("state.db");
        let app = App::open(&database).unwrap();
        let paths = app.plugin_paths("caricature").unwrap();
        let store = ArtworkStore::new(paths.data.join("artwork"));
        let art = Artwork {
            id: "protected-1".into(),
            metadata: ArtworkMetadata {
                job_id: "protected".into(),
                slot: 0,
                created_at_unix: 0,
                input: GenerationInput::News,
                source_urls: vec![],
                news_summary: None,
                settings: GenerationSettings::default(),
            },
            original_sha256: None,
            original_codec: None,
            eink_png: crate::caricature::test_canonical_png(),
            lcd_rgb565be: vec![0; 160 * 80 * 2],
        };
        store.publish(art, 0).unwrap();
        let router = router(Arc::new(Mutex::new(app)));
        let route = "/api/plugins/caricature/protections";
        assert_eq!(
            call(&router, Method::GET, route, json!({})).await.1["revision"],
            0
        );
        let changed = json!({"expected_revision":0,"artwork_ids":["protected-1"],"protect":true});
        let (status, value) = call(&router, Method::PUT, route, changed.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["protected_ids"][0], "protected-1");
        assert_eq!(
            call(&router, Method::PUT, route, changed).await.0,
            StatusCode::CONFLICT
        );
        let unknown = json!({"expected_revision":1,"artwork_ids":["unknown"],"protect":true});
        assert_eq!(
            call(&router, Method::PUT, route, unknown).await.0,
            StatusCode::BAD_REQUEST
        );
        drop(router);
        let reopened = super::router(Arc::new(Mutex::new(App::open(&database).unwrap())));
        assert_eq!(
            call(&reopened, Method::GET, route, json!({})).await.1["protected_ids"][0],
            "protected-1"
        );
    }

    #[tokio::test]
    async fn caricature_web_preview_uses_saved_webp_and_legacy_png_only() {
        use crate::caricature::originals::half_webp_quality90;
        use crate::caricature::store::ArtworkStore;
        use crate::caricature::{
            Artwork, ArtworkMetadata, CaricatureContent, GenerationInput, GenerationSettings,
            HistoryEntry, OriginalCodec,
        };
        use sha2::Digest;
        let temp = tempfile::tempdir().unwrap();
        let mut app = App::open(&temp.path().join("state.db")).unwrap();
        let paths = app.plugin_paths("caricature").unwrap();
        let store = ArtworkStore::new(paths.data.join("artwork"));
        let canonical = crate::caricature::test_canonical_png();
        let artwork = |id: &str| Artwork {
            id: id.into(),
            metadata: ArtworkMetadata {
                job_id: id.into(),
                slot: 0,
                created_at_unix: 0,
                input: GenerationInput::News,
                source_urls: vec![],
                news_summary: None,
                settings: GenerationSettings::default(),
            },
            original_sha256: None,
            original_codec: None,
            eink_png: canonical.clone(),
            lcd_rgb565be: vec![0; 160 * 80 * 2],
        };
        let legacy = store.publish(artwork("legacy-1"), 1).unwrap();
        let mut recent = artwork("recent-1");
        let original = half_webp_quality90(&canonical).unwrap();
        recent.original_sha256 = Some(format!("{:x}", sha2::Sha256::digest(&original)));
        recent.original_codec = Some(OriginalCodec::HalfWebpQ90);
        let recent = store.publish_with_original(recent, &original, 2).unwrap();
        let mut snapshot = serde_json::to_value(CaricatureContent::default()).unwrap();
        snapshot["history"] = serde_json::to_value(
            [&legacy, &recent]
                .iter()
                .map(|p| HistoryEntry {
                    artwork_id: p.artwork.id.clone(),
                    metadata: p.artwork.metadata.clone(),
                    publish_seq: p.publish_seq,
                    published_at_unix: p.published_at_unix,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut content = CaricatureContent::default();
        crate::content::ContentPlugin::load_state(&mut content, snapshot).unwrap();
        app.replace_test_content("caricature", Box::new(content));
        let router = router(Arc::new(Mutex::new(app)));
        for (id, mime, expected) in [
            ("recent-1", "image/webp", original.as_slice()),
            ("legacy-1", "image/png", canonical.as_slice()),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/plugins/caricature/originals/{id}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[axum::http::header::CONTENT_TYPE], mime);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(bytes.as_ref(), expected);
            let dimensions = image::load_from_memory(&bytes).unwrap();
            let expected_size = if id == "recent-1" {
                (125, 61)
            } else {
                (250, 122)
            };
            assert_eq!((dimensions.width(), dimensions.height()), expected_size);
        }
        std::fs::remove_file(paths.data.join("artwork/archive/recent-1.original.webp")).unwrap();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/plugins/caricature/originals/recent-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/plugins/caricature/originals/unknown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
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

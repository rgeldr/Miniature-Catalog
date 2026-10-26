use crate::auth::{self, Scope};
use crate::db::{self, MiniFilter, NewLocation};
use crate::desktop;
use crate::error::ApiError;
use crate::prep::{self, Need};
use crate::state::AppState;
use crate::thumb;
use axum::extract::{DefaultBodyLimit, FromRequest, Multipart, Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, PathBuf};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

const OPENAPI: &str = include_str!("../openapi.yaml");
const MAX_IMAGE: usize = 8 * 1024 * 1024;

pub fn router(state: AppState, cors_origins: &[HeaderValue]) -> Router {
    build(state, cors_origins, false)
}

/// Window listener. Same API as [`router`], plus the unauthenticated session
/// route that hands the admin token to the desktop UI. Do not serve this on
/// the encounter-generator port.
pub fn ui_router(state: AppState) -> Router {
    build(state, &[], true)
}

fn build(state: AppState, cors_origins: &[HeaderValue], include_session: bool) -> Router {
    let read = Router::new()
        .route("/api/v1/miniatures", get(list_miniatures))
        .route("/api/v1/miniatures/:id", get(get_miniature))
        .route("/api/v1/miniatures/:id/thumbnail", get(get_thumbnail))
        .route("/api/v1/tags", get(list_tags))
        .route("/api/v1/locations", get(list_locations))
        .route("/api/v1/reports/locations", get(location_report))
        .route("/api/v1/facets", get(facets))
        .route("/api/v1/prep", post(prep))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_read));

    let admin = Router::new()
        .route("/api/v1/miniatures", post(create_miniature))
        .route(
            "/api/v1/miniatures/:id",
            axum::routing::put(update_miniature).delete(delete_miniature),
        )
        .route("/api/v1/miniatures/:id/thumbnail", post(upload_thumbnail))
        .route(
            "/api/v1/miniatures/:id/thumbnail/import",
            post(import_thumbnail),
        )
        .route("/api/v1/locations", post(create_location))
        .route(
            "/api/v1/locations/:id",
            axum::routing::put(update_location).delete(delete_location),
        )
        .route("/api/v1/tags", post(create_tag))
        .route("/api/v1/api-keys", get(list_keys).post(create_key))
        .route("/api/v1/api-keys/:id", axum::routing::delete(revoke_key))
        .route(
            "/api/v1/settings",
            axum::routing::get(get_settings).put(put_settings),
        )
        .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));

    let mut app = Router::new()
        .route("/health", get(health))
        .route("/openapi.yaml", get(openapi_doc));
    if include_session {
        app = app.route("/api/v1/desktop-session", get(desktop_session));
    }
    let mut app = app
        .merge(read)
        .merge(admin)
        .fallback(fallback)
        .layer(DefaultBodyLimit::max(MAX_IMAGE))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .with_state(state);

    if !cors_origins.is_empty() {
        let cors = CorsLayer::new()
            .allow_origin(AllowOrigin::list(cors_origins.to_vec()))
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
        app = app.layer(cors);
    }
    app
}

async fn require_read(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let scope = authenticate(&state, request.headers())?;
    if !scope.allows(Scope::Read) {
        return Err(ApiError::forbidden());
    }
    Ok(next.run(request).await)
}

async fn require_admin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let scope = authenticate(&state, request.headers())?;
    if !scope.allows(Scope::Admin) {
        return Err(ApiError::forbidden());
    }
    Ok(next.run(request).await)
}

fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<Scope, ApiError> {
    let token = bearer(headers).ok_or_else(ApiError::unauthorized)?;
    if auth::token_matches(&token, &state.admin_hash) {
        return Ok(Scope::Admin);
    }
    let hash = auth::hash_token(&token);
    let conn = state.lock()?;
    match db::active_scope(&conn, &hash)? {
        Some(scope) => Scope::parse(&scope).ok_or_else(ApiError::unauthorized),
        None => Err(ApiError::unauthorized()),
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn openapi_doc() -> Response {
    (
        [(header::CONTENT_TYPE, "application/yaml; charset=utf-8")],
        OPENAPI,
    )
        .into_response()
}

async fn fallback(State(state): State<AppState>, uri: axum::http::Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return ApiError::not_found().into_response();
    }
    let Some(root) = state.static_dir.clone() else {
        return ApiError::not_found().into_response();
    };
    let rel = uri.path().trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let Some(path) = safe_join(&root, rel) else {
        return ApiError::not_found().into_response();
    };
    let path = if path.is_dir() {
        path.join("index.html")
    } else {
        path
    };
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_of(&path);
            ([(header::CONTENT_TYPE, mime)], bytes).into_response()
        }
        Err(_) => ApiError::not_found().into_response(),
    }
}

fn safe_join(root: &std::path::Path, rel: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for part in std::path::Path::new(rel).components() {
        match part {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn mime_of(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

#[derive(Deserialize)]
struct MiniQuery {
    q: Option<String>,
    race: Option<String>,
    role: Option<String>,
    combat: Option<String>,
    size: Option<String>,
    game: Option<String>,
    location_id: Option<String>,
}

impl MiniQuery {
    fn into_filter(self) -> Result<MiniFilter, ApiError> {
        let mut location_ids = Vec::new();
        for part in split_csv(self.location_id) {
            let id: i64 = part.parse().map_err(|_| {
                ApiError::bad("location_id must be a comma-separated list of integers")
            })?;
            location_ids.push(id);
        }
        Ok(MiniFilter {
            name: self.q,
            races: split_csv(self.race),
            roles: split_csv(self.role),
            combat: split_csv(self.combat),
            sizes: split_csv(self.size),
            games: split_csv(self.game),
            location_ids,
        })
    }
}

fn split_csv(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

async fn list_miniatures(
    State(state): State<AppState>,
    Query(query): Query<MiniQuery>,
) -> Result<Json<Value>, ApiError> {
    let filter = query.into_filter()?;
    let conn = state.lock()?;
    let miniatures = db::list_miniatures(&conn, &filter)?;
    Ok(Json(json!({ "miniatures": miniatures })))
}

async fn get_miniature(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<db::Miniature>, ApiError> {
    let conn = state.lock()?;
    db::get_miniature(&conn, id)?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn list_tags(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.lock()?;
    Ok(Json(json!({ "tags": db::list_tags(&conn)? })))
}

async fn list_locations(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.lock()?;
    Ok(Json(json!({ "locations": db::list_locations(&conn)? })))
}

async fn location_report(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.lock()?;
    Ok(Json(json!({ "locations": db::location_report(&conn)? })))
}

async fn facets(State(state): State<AppState>) -> Result<Json<db::Facets>, ApiError> {
    let conn = state.lock()?;
    Ok(Json(db::facets(&conn)?))
}

#[derive(Deserialize)]
struct PrepBody {
    requests: Vec<Need>,
}

#[derive(Serialize)]
struct PrepResponse {
    groups: Vec<prep::LocationGroup>,
    requests: Vec<prep::RequestResult>,
    shortages: Vec<prep::Shortage>,
}

async fn prep(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<PrepBody>,
) -> Result<Json<PrepResponse>, ApiError> {
    validate_prep(&body.requests)?;
    let conn = state.lock()?;
    let stock = db::stock(&conn)?;
    let list = prep::build_prep_list(&stock, &body.requests);
    let shortages = list.shortages();
    Ok(Json(PrepResponse {
        groups: list.groups,
        requests: list.requests,
        shortages,
    }))
}

fn validate_prep(requests: &[Need]) -> Result<(), ApiError> {
    if requests.is_empty() || requests.len() > 50 {
        return Err(ApiError::bad("requests must contain 1 to 50 items"));
    }
    for (index, need) in requests.iter().enumerate() {
        if !(1..=10_000).contains(&need.quantity) {
            return Err(ApiError::bad(format!(
                "request {index} quantity must be from 1 to 10000"
            )));
        }
        if need.label.chars().count() > 200 {
            return Err(ApiError::bad(format!("request {index} label is too long")));
        }
        if !prep::need_has_criteria(need) {
            return Err(ApiError::bad(format!(
                "request {index} needs a name, tag, race, role, combat style, size, or game"
            )));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct LocationBody {
    name: String,
    kind: String,
    parent_id: Option<i64>,
}

impl LocationBody {
    fn into_new(self) -> NewLocation {
        NewLocation {
            name: self.name,
            kind: self.kind,
            parent_id: self.parent_id,
        }
    }
}

async fn create_location(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<LocationBody>,
) -> Result<(StatusCode, Json<db::Location>), ApiError> {
    let conn = state.lock()?;
    let location = db::create_location(&conn, body.into_new())?;
    Ok((StatusCode::CREATED, Json(location)))
}

async fn update_location(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    JsonBody(body): JsonBody<LocationBody>,
) -> Result<Json<db::Location>, ApiError> {
    let conn = state.lock()?;
    Ok(Json(db::update_location(&conn, id, body.into_new())?))
}

async fn delete_location(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let conn = state.lock()?;
    db::delete_location(&conn, id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TagBody {
    category: String,
    name: String,
}

async fn create_tag(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<TagBody>,
) -> Result<(StatusCode, Json<db::Tag>), ApiError> {
    let conn = state.lock()?;
    let tag = db::create_tag(&conn, &body.category, &body.name)?;
    Ok((StatusCode::CREATED, Json(tag)))
}

#[derive(Deserialize)]
struct MiniatureBody {
    name: String,
    #[serde(default)]
    notes: String,
    quantity: u32,
    size: String,
    #[serde(default)]
    game: Option<String>,
    location_id: i64,
    #[serde(default)]
    races: Vec<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    combat: Vec<String>,
}

impl MiniatureBody {
    fn into_new(self) -> db::NewMiniature {
        db::NewMiniature {
            name: self.name,
            notes: self.notes,
            quantity: self.quantity,
            size: self.size,
            game: self.game,
            location_id: self.location_id,
            races: self.races,
            roles: self.roles,
            combat: self.combat,
        }
    }
}

async fn create_miniature(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<MiniatureBody>,
) -> Result<(StatusCode, Json<db::Miniature>), ApiError> {
    let mini = db::validate_miniature(body.into_new())?;
    let conn = state.lock()?;
    let created = db::create_miniature(&conn, mini)?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_miniature(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    JsonBody(body): JsonBody<MiniatureBody>,
) -> Result<Json<db::Miniature>, ApiError> {
    let mini = db::validate_miniature(body.into_new())?;
    let conn = state.lock()?;
    Ok(Json(db::update_miniature(&conn, id, mini)?))
}

async fn delete_miniature(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let path = state.thumbs_dir.join(format!("{id}.jpg"));
    {
        let conn = state.lock()?;
        db::delete_miniature(&conn, id)?;
    }
    let _ = tokio::fs::remove_file(path).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_thumbnail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let file = {
        let conn = state.lock()?;
        let mini = db::get_miniature(&conn, id)?.ok_or_else(ApiError::not_found)?;
        if mini.thumbnail_url.is_none() {
            return Err(ApiError::not_found());
        }
        format!("{id}.jpg")
    };
    let bytes = tokio::fs::read(state.thumbs_dir.join(file))
        .await
        .map_err(|_| ApiError::not_found())?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Serialize)]
struct ThumbRef {
    thumbnail_url: String,
}

async fn upload_thumbnail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    request: Request,
) -> Result<Json<ThumbRef>, ApiError> {
    ensure_miniature(&state, id)?;
    let bytes = read_image_bytes(request).await?;
    store_thumbnail(&state, id, &bytes).await?;
    Ok(Json(ThumbRef {
        thumbnail_url: format!("/api/v1/miniatures/{id}/thumbnail"),
    }))
}

#[derive(Deserialize)]
struct ImportBody {
    source: String,
}

async fn import_thumbnail(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    JsonBody(body): JsonBody<ImportBody>,
) -> Result<Json<ThumbRef>, ApiError> {
    ensure_miniature(&state, id)?;
    let file_id = thumb::parse_drive_file_id(&body.source).map_err(ApiError::bad)?;
    let bytes = download_drive_file(&state, &file_id).await?;
    store_thumbnail(&state, id, &bytes).await?;
    Ok(Json(ThumbRef {
        thumbnail_url: format!("/api/v1/miniatures/{id}/thumbnail"),
    }))
}

fn ensure_miniature(state: &AppState, id: i64) -> Result<(), ApiError> {
    let conn = state.lock()?;
    if db::get_miniature(&conn, id)?.is_none() {
        return Err(ApiError::not_found());
    }
    Ok(())
}

async fn store_thumbnail(state: &AppState, id: i64, bytes: &[u8]) -> Result<(), ApiError> {
    if bytes.len() > MAX_IMAGE {
        return Err(ApiError::bad("image is larger than 8MB"));
    }
    let owned = bytes.to_vec();
    let jpeg = tokio::task::spawn_blocking(move || thumb::jpeg_thumbnail(&owned))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(ApiError::bad)?;
    let file = format!("{id}.jpg");
    tokio::fs::write(state.thumbs_dir.join(&file), &jpeg)
        .await
        .map_err(|_| ApiError::internal())?;
    let conn = state.lock()?;
    db::set_thumbnail(&conn, id, &file)?;
    Ok(())
}

async fn read_image_bytes(request: Request) -> Result<Vec<u8>, ApiError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    if content_type.starts_with("image/") {
        let bytes = axum::body::to_bytes(request.into_body(), MAX_IMAGE)
            .await
            .map_err(|_| ApiError::bad("could not read image body"))?;
        return Ok(bytes.to_vec());
    }
    let mut multipart = Multipart::from_request(request, &()).await.map_err(|_| {
        ApiError::bad("expected an image body or a multipart form with a file field")
    })?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| ApiError::bad("invalid multipart body"))?
    {
        if field.name() == Some("file") {
            let bytes = field
                .bytes()
                .await
                .map_err(|_| ApiError::bad("could not read upload"))?;
            return Ok(bytes.to_vec());
        }
    }
    Err(ApiError::bad("multipart form needs a file field"))
}

async fn download_drive_file(state: &AppState, file_id: &str) -> Result<Vec<u8>, ApiError> {
    let url = format!("https://drive.google.com/uc?export=download&id={file_id}&confirm=t");
    let response = state
        .http
        .get(&url)
        .send()
        .await
        .map_err(|_| ApiError::bad("could not download that Drive file"))?;
    if !response.status().is_success() {
        return Err(ApiError::bad(
            "Drive did not return the file. Share it as anyone with the link.",
        ));
    }
    let host = response.url().host_str().unwrap_or("");
    if !thumb::google_download_host_ok(host) {
        return Err(ApiError::bad(
            "Drive download redirected to an unexpected host",
        ));
    }
    if let Some(length) = response.content_length() {
        if length > MAX_IMAGE as u64 {
            return Err(ApiError::bad("image is larger than 8MB"));
        }
    }
    let mut response = response;
    let mut buf = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ApiError::bad("could not read the Drive download"))?
    {
        if buf.len() + chunk.len() > MAX_IMAGE {
            return Err(ApiError::bad("image is larger than 8MB"));
        }
        buf.extend_from_slice(&chunk);
    }
    let trimmed = buf
        .iter()
        .take(64)
        .cloned()
        .collect::<Vec<_>>()
        .to_ascii_lowercase();
    if trimmed.starts_with(b"<!doctype") || trimmed.starts_with(b"<html") {
        return Err(ApiError::bad(
            "Drive returned a web page instead of an image. Share the file as anyone with the link, or upload the image directly.",
        ));
    }
    Ok(buf)
}

#[derive(Deserialize)]
struct KeyBody {
    name: String,
    scope: String,
}

#[derive(Serialize)]
struct CreatedKey {
    id: i64,
    name: String,
    scope: String,
    token: String,
}

async fn list_keys(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.lock()?;
    Ok(Json(json!({ "api_keys": db::list_api_keys(&conn)? })))
}

async fn create_key(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<KeyBody>,
) -> Result<(StatusCode, Json<CreatedKey>), ApiError> {
    let scope = Scope::parse(body.scope.trim())
        .ok_or_else(|| ApiError::bad("scope must be read or admin"))?;
    let token = auth::generate_token();
    let hash = auth::hash_token(&token);
    let conn = state.lock()?;
    let id = db::insert_api_key(&conn, &body.name, &hash, scope.as_str())?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedKey {
            id,
            name: body.name.trim().to_string(),
            scope: scope.as_str().to_string(),
            token,
        }),
    ))
}

async fn revoke_key(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    let conn = state.lock()?;
    db::revoke_api_key(&conn, id)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn desktop_session(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "token": state.admin_token }))
}

#[derive(Deserialize)]
struct SettingsBody {
    api_enabled: bool,
    api_bind: String,
}

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(settings_view(&state).await?))
}

async fn put_settings(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    let settings = desktop::Settings {
        api_enabled: body.api_enabled,
        api_bind: body.api_bind,
    };
    desktop::validate_api_bind(&settings.api_bind).map_err(ApiError::bad)?;
    if let Some(host) = state.host.clone() {
        let previous = desktop::load_settings(&state.data_dir).map_err(internal_message)?;
        if let Err(err) = host.apply(state.clone(), &settings).await {
            if previous.api_enabled {
                let _ = host.apply(state.clone(), &previous).await;
            }
            return Err(ApiError::bad(err));
        }
    }
    desktop::save_settings(&state.data_dir, &settings).map_err(internal_message)?;
    Ok(Json(settings_view(&state).await?))
}

async fn settings_view(state: &AppState) -> Result<Value, ApiError> {
    let settings = desktop::load_settings(&state.data_dir).map_err(internal_message)?;
    let bound = match &state.host {
        Some(host) => host.bound_addr().await.map(|addr| addr.to_string()),
        None => None,
    };
    Ok(json!({
        "api_enabled": settings.api_enabled,
        "api_bind": settings.api_bind,
        "listening": bound.is_some(),
        "bound": bound,
        "managed": state.host.is_some(),
    }))
}

fn internal_message(err: String) -> ApiError {
    eprintln!("settings error: {err}");
    ApiError::internal()
}

struct JsonBody<T>(T);

#[axum::async_trait]
impl<T, S> FromRequest<S> for JsonBody<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(JsonBody(value)),
            Err(err) => Err(ApiError::bad(format!("invalid JSON: {err}"))),
        }
    }
}

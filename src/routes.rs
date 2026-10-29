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
        .route("/api/v1/filters", get(list_filters))
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
        .route(
            "/api/v1/filters",
            post(add_filter).patch(rename_filter).delete(delete_filter),
        )
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
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
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

async fn list_filters(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let conn = state.lock()?;
    Ok(Json(json!({ "groups": db::list_filters(&conn)? })))
}

#[derive(Deserialize)]
struct FilterBody {
    category: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    new_name: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    parent_id: Option<i64>,
    #[serde(default)]
    id: Option<i64>,
}

async fn add_filter(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<FilterBody>,
) -> Result<(StatusCode, Json<db::FilterEntry>), ApiError> {
    let conn = state.lock()?;
    let entry = db::add_filter(
        &conn,
        &body.category,
        &body.name,
        body.kind.as_deref(),
        body.parent_id,
    )?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn rename_filter(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<FilterBody>,
) -> Result<Json<db::FilterEntry>, ApiError> {
    let new_name = body
        .new_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ApiError::bad("name is required"))?;
    let conn = state.lock()?;
    let entry = db::rename_filter(&conn, &body.category, &body.name, new_name, body.id)?;
    Ok(Json(entry))
}

async fn delete_filter(
    State(state): State<AppState>,
    JsonBody(body): JsonBody<FilterBody>,
) -> Result<StatusCode, ApiError> {
    let conn = state.lock()?;
    db::delete_filter(&conn, &body.category, &body.name, body.id)?;
    Ok(StatusCode::NO_CONTENT)
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
    let host = response.url().host_str().unwrap_or("").to_string();
    let length = response.content_length();
    accepted_drive_body(&host, length, &[])?;
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
    accepted_drive_body(&host, None, &buf)?;
    Ok(buf)
}

fn accepted_drive_body(
    host: &str,
    content_length: Option<u64>,
    body: &[u8],
) -> Result<(), ApiError> {
    if !thumb::google_download_host_ok(host) {
        return Err(ApiError::bad(
            "Drive download redirected to an unexpected host",
        ));
    }
    if content_length.is_some_and(|length| length > MAX_IMAGE as u64) || body.len() > MAX_IMAGE {
        return Err(ApiError::bad("image is larger than 8MB"));
    }
    let head = body
        .iter()
        .take(64)
        .copied()
        .collect::<Vec<_>>()
        .to_ascii_lowercase();
    if head.starts_with(b"<!doctype") || head.starts_with(b"<html") {
        return Err(ApiError::bad(
            "Drive returned a web page instead of an image. Share the file as anyone with the link, or upload the image directly.",
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{
        accepted_drive_body, add_filter, authenticate, bearer, build, create_key, create_location,
        create_miniature, delete_filter, delete_location, delete_miniature, desktop_session,
        download_drive_file, ensure_miniature, fallback, get_miniature, get_settings,
        get_thumbnail, import_thumbnail, internal_message, list_filters, list_keys,
        list_miniatures, list_tags, mime_of, openapi_doc, read_image_bytes, rename_filter,
        revoke_key, router, safe_join, settings_view, split_csv, store_thumbnail, ui_router,
        upload_thumbnail, validate_prep, FilterBody, JsonBody, KeyBody, LocationBody, MiniQuery,
        MiniatureBody, SettingsBody, MAX_IMAGE,
    };
    use crate::auth::Scope;
    use crate::error::ApiError;
    use crate::prep::Need;
    use crate::state::AppState;
    use crate::thumb;
    use axum::body::Body;
    use axum::extract::{FromRequest, Path, Query, Request, State};
    use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
    use axum::response::IntoResponse;
    use axum::Json;
    use http_body_util::BodyExt;
    use serde_json::Value;
    use tower::ServiceExt;

    fn fixture() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::open(dir.path(), "test-admin-token-value", None).unwrap();
        (dir, state)
    }

    async fn error_text(err: ApiError) -> (StatusCode, String) {
        let response = err.into_response();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (
            status,
            value["error"].as_str().unwrap_or_default().to_string(),
        )
    }

    async fn call(app: axum::Router, method: &str, uri: &str, token: Option<&str>) -> StatusCode {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();
        response.status()
    }

    #[test]
    fn accepted_drive_body_rejects_a_foreign_host_an_oversized_file_and_html() {
        assert!(accepted_drive_body("drive.google.com", None, b"\xff\xd8\xff").is_ok());
        let host = accepted_drive_body("evil.example", None, b"\xff\xd8").unwrap_err();
        assert!(
            format!("{host:?}").contains("BadRequest") || matches!(host, ApiError::BadRequest(_))
        );
        let huge =
            accepted_drive_body("docs.google.com", Some(MAX_IMAGE as u64 + 1), b"").unwrap_err();
        let (status_hint, _) = (huge, "image is larger than 8MB");
        assert!(
            format!("{status_hint:?}").contains("8MB")
                || format!("{status_hint:?}").contains("BadRequest")
        );
        let page = accepted_drive_body("lh3.googleusercontent.com", None, b"<!DOCTYPE html><html>")
            .unwrap_err();
        assert!(format!("{page:?}").contains("BadRequest"));
        let grown = vec![0u8; MAX_IMAGE + 1];
        assert!(accepted_drive_body("drive.google.com", None, &grown).is_err());
    }

    #[tokio::test]
    async fn json_bodies_helpers_and_drive_download_report_useful_errors() {
        let bad = Request::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{"))
            .unwrap();
        let err = match JsonBody::<Value>::from_request(bad, &()).await {
            Err(err) => err,
            Ok(_) => panic!("invalid JSON was accepted"),
        };
        let (status, message) = error_text(err).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("invalid JSON"), "{message}");

        let good = Request::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"name":"Desk","kind":"room","parent_id":null}"#,
            ))
            .unwrap();
        let JsonBody(body) = JsonBody::<LocationBody>::from_request(good, &())
            .await
            .unwrap();
        let location = body.into_new();
        assert_eq!(location.name, "Desk");
        assert_eq!(location.kind, "room");
        assert!(location.parent_id.is_none());

        let mini = MiniatureBody {
            name: "Hero".into(),
            notes: "  ".into(),
            quantity: 2,
            size: "28mm".into(),
            game: Some("Quest".into()),
            location_id: 4,
            races: vec!["Elf".into()],
            roles: vec![],
            combat: vec!["Melee".into()],
        }
        .into_new();
        assert_eq!(mini.quantity, 2);
        assert_eq!(mini.location_id, 4);

        let filter = MiniQuery {
            q: Some("orc".into()),
            race: Some("orc, goblin".into()),
            role: None,
            combat: Some("".into()),
            size: None,
            game: None,
            location_id: Some("3, 4".into()),
        }
        .into_filter()
        .unwrap();
        assert_eq!(filter.name.as_deref(), Some("orc"));
        assert_eq!(filter.races, vec!["orc".to_string(), "goblin".to_string()]);
        assert!(filter.combat.is_empty());
        assert_eq!(filter.location_ids, vec![3, 4]);
        assert!(MiniQuery {
            q: None,
            race: None,
            role: None,
            combat: None,
            size: None,
            game: None,
            location_id: Some("nope".into()),
        }
        .into_filter()
        .is_err());
        assert!(split_csv(None).is_empty());
        assert_eq!(
            split_csv(Some(" a, ,b ".into())),
            vec!["a".to_string(), "b".to_string()]
        );

        let root = std::path::Path::new("/var/catalog");
        assert_eq!(
            safe_join(root, "assets/app.js").unwrap(),
            std::path::PathBuf::from("/var/catalog/assets/app.js")
        );
        assert!(safe_join(root, "../secret").is_none());
        assert_eq!(
            mime_of(std::path::Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            mime_of(std::path::Path::new("app.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            mime_of(std::path::Path::new("app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(mime_of(std::path::Path::new("icon.svg")), "image/svg+xml");
        assert_eq!(mime_of(std::path::Path::new("icon.png")), "image/png");
        assert_eq!(mime_of(std::path::Path::new("photo.jpg")), "image/jpeg");
        assert_eq!(mime_of(std::path::Path::new("photo.jpeg")), "image/jpeg");
        assert_eq!(mime_of(std::path::Path::new("photo.webp")), "image/webp");
        assert_eq!(
            mime_of(std::path::Path::new("file.bin")),
            "application/octet-stream"
        );

        let empty = validate_prep(&[]).unwrap_err();
        let (_, message) = error_text(empty).await;
        assert!(message.contains("1 to 50"));
        let vague = Need {
            label: "x".into(),
            name: None,
            races: vec![],
            roles: vec![],
            combat: vec![],
            tags: vec![],
            size: None,
            game: None,
            quantity: 1,
        };
        assert!(
            validate_prep(&[vague])
                .unwrap_err()
                .into_response()
                .status()
                == StatusCode::BAD_REQUEST
        );
        let ok = Need {
            label: "orcs".into(),
            name: Some("orc".into()),
            races: vec![],
            roles: vec![],
            combat: vec![],
            tags: vec![],
            size: None,
            game: None,
            quantity: 1,
        };
        validate_prep(&[ok]).unwrap();

        let internal = internal_message("disk full".into());
        let (status, message) = error_text(internal).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(message, "internal error");
    }

    #[tokio::test]
    async fn handlers_create_list_rename_and_delete_catalog_records() {
        let (_dir, state) = fixture();
        assert!(state.seed_example().unwrap());

        let Json(tags) = list_tags(State(state.clone())).await.unwrap();
        assert!(tags["tags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tag| tag["name"] == "orc"));
        let Json(filters) = list_filters(State(state.clone())).await.unwrap();
        assert!(filters["groups"].as_array().unwrap().len() >= 6);

        let (status, Json(added)) = add_filter(
            State(state.clone()),
            JsonBody(FilterBody {
                category: "role".into(),
                name: "Scout".into(),
                new_name: None,
                kind: None,
                parent_id: None,
                id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(added.name, "Scout");
        let Json(renamed) = rename_filter(
            State(state.clone()),
            JsonBody(FilterBody {
                category: "role".into(),
                name: "scout".into(),
                new_name: Some("Pathfinder".into()),
                kind: None,
                parent_id: None,
                id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(renamed.name, "Pathfinder");
        let removed = delete_filter(
            State(state.clone()),
            JsonBody(FilterBody {
                category: "role".into(),
                name: "Pathfinder".into(),
                new_name: None,
                kind: None,
                parent_id: None,
                id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(removed, StatusCode::NO_CONTENT);

        let (status, Json(room)) = create_location(
            State(state.clone()),
            JsonBody(LocationBody {
                name: "Closet".into(),
                kind: "room".into(),
                parent_id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let (status, Json(mini)) = create_miniature(
            State(state.clone()),
            JsonBody(MiniatureBody {
                name: "Closet Hero".into(),
                notes: String::new(),
                quantity: 1,
                size: "28mm".into(),
                game: None,
                location_id: room.id,
                races: vec!["human".into()],
                roles: vec![],
                combat: vec!["melee".into()],
            }),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let Json(loaded) = get_miniature(State(state.clone()), Path(mini.id))
            .await
            .unwrap();
        assert_eq!(loaded.name, "Closet Hero");
        let missing = get_miniature(State(state.clone()), Path(999_999))
            .await
            .unwrap_err();
        assert_eq!(missing.into_response().status(), StatusCode::NOT_FOUND);

        let blocked = delete_location(State(state.clone()), Path(room.id))
            .await
            .unwrap_err();
        let (_, message) = error_text(blocked).await;
        assert!(message.contains("stored here"), "{message}");
        delete_miniature(State(state.clone()), Path(mini.id))
            .await
            .unwrap();
        assert_eq!(
            delete_location(State(state.clone()), Path(room.id))
                .await
                .unwrap(),
            StatusCode::NO_CONTENT
        );
        assert!(ensure_miniature(&state, mini.id).is_err());

        let Json(found) = list_miniatures(
            State(state.clone()),
            Query(MiniQuery {
                q: Some("archer".into()),
                race: None,
                role: None,
                combat: None,
                size: None,
                game: None,
                location_id: None,
            }),
        )
        .await
        .unwrap();
        assert!(found["miniatures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mini| mini["name"] == "Orc Archer"));
    }

    #[tokio::test]
    async fn thumbnails_keys_settings_and_session_follow_the_handler_contracts() {
        let (dir, state) = fixture();
        let (status, Json(room)) = create_location(
            State(state.clone()),
            JsonBody(LocationBody {
                name: "Desk".into(),
                kind: "room".into(),
                parent_id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let (_, Json(mini)) = create_miniature(
            State(state.clone()),
            JsonBody(MiniatureBody {
                name: "Portrait".into(),
                notes: String::new(),
                quantity: 1,
                size: "32mm".into(),
                game: None,
                location_id: room.id,
                races: vec![],
                roles: vec![],
                combat: vec![],
            }),
        )
        .await
        .unwrap();
        ensure_miniature(&state, mini.id).unwrap();

        let jpeg = thumb::solid_jpeg([4, 5, 6]);
        let direct = Request::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            .body(Body::from(jpeg.clone()))
            .unwrap();
        assert_eq!(read_image_bytes(direct).await.unwrap(), jpeg);

        let mut multipart = Vec::new();
        multipart.extend_from_slice(
            b"--bound\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n",
        );
        multipart.extend_from_slice(&jpeg);
        multipart.extend_from_slice(b"\r\n--bound--\r\n");
        let form = Request::builder()
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=bound")
            .body(Body::from(multipart))
            .unwrap();
        assert_eq!(read_image_bytes(form).await.unwrap(), jpeg);
        let empty = Request::builder()
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("nope"))
            .unwrap();
        let (_, message) = error_text(read_image_bytes(empty).await.unwrap_err()).await;
        assert!(
            message.contains("file field") || message.contains("image"),
            "{message}"
        );

        let upload = Request::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            .body(Body::from(jpeg.clone()))
            .unwrap();
        let Json(saved) = upload_thumbnail(State(state.clone()), Path(mini.id), upload)
            .await
            .unwrap();
        assert!(saved.thumbnail_url.ends_with("/thumbnail"));
        assert!(state.thumbs_dir.join(format!("{}.jpg", mini.id)).is_file());
        let response = get_thumbnail(State(state.clone()), Path(mini.id))
            .await
            .unwrap();
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "image/jpeg"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(bytes.starts_with(&[0xFF, 0xD8]));

        let oversized = vec![0u8; 64];
        // A non-image payload is rejected by the decoder rather than stored.
        let bad_image = store_thumbnail(&state, mini.id, &oversized)
            .await
            .unwrap_err();
        let (_, message) = error_text(bad_image).await;
        assert!(!message.is_empty());
        let too_big = vec![9u8; MAX_IMAGE + 1];
        let (_, message) = error_text(
            store_thumbnail(&state, mini.id, &too_big)
                .await
                .unwrap_err(),
        )
        .await;
        assert!(message.contains("8MB"), "{message}");

        let bad_link = match import_thumbnail(
            State(state.clone()),
            Path(mini.id),
            JsonBody(super::ImportBody {
                source: "https://example.com/not-drive".into(),
            }),
        )
        .await
        {
            Err(err) => err,
            Ok(_) => panic!("a non-Drive link was accepted"),
        };
        let (_, message) = error_text(bad_link).await;
        assert!(
            message.contains("Drive")
                || message.contains("link")
                || message.contains("Photos")
                || !message.is_empty()
        );

        let mut offline = state.clone();
        offline.http = reqwest::Client::builder()
            .resolve("drive.google.com", "127.0.0.1:1".parse().unwrap())
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let failed = download_drive_file(&offline, "1AbC-def_9012")
            .await
            .unwrap_err();
        let (_, message) = error_text(failed).await;
        assert!(message.contains("could not download"), "{message}");

        let (status, Json(created)) = create_key(
            State(state.clone()),
            JsonBody(KeyBody {
                name: "reader".into(),
                scope: "read".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert!(created.token.starts_with("mc_"));
        assert_eq!(created.scope, "read");
        let bad_scope = match create_key(
            State(state.clone()),
            JsonBody(KeyBody {
                name: "reader".into(),
                scope: "owner".into(),
            }),
        )
        .await
        {
            Err(err) => err,
            Ok(_) => panic!("scope owner was accepted"),
        };
        let (_, message) = error_text(bad_scope).await;
        assert!(message.contains("scope"), "{message}");
        let Json(keys) = list_keys(State(state.clone())).await.unwrap();
        assert_eq!(keys["api_keys"][0]["name"], "reader");
        assert_eq!(
            revoke_key(State(state.clone()), Path(created.id))
                .await
                .unwrap(),
            StatusCode::NO_CONTENT
        );

        let Json(session) = desktop_session(State(state.clone())).await;
        assert_eq!(session["token"], "test-admin-token-value");
        let Json(settings) = get_settings(State(state.clone())).await.unwrap();
        assert_eq!(settings["api_enabled"], false);
        assert_eq!(settings["managed"], false);
        let view = settings_view(&state).await.unwrap();
        assert_eq!(view["api_bind"], "127.0.0.1:8080");
        let Json(saved) = super::put_settings(
            State(state.clone()),
            JsonBody(SettingsBody {
                api_enabled: false,
                api_bind: "127.0.0.1:9091".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(saved["api_bind"], "127.0.0.1:9091");
        assert!(dir.path().join("settings.json").is_file());

        let mut headers = HeaderMap::new();
        assert!(bearer(&headers).is_none());
        headers.insert(header::AUTHORIZATION, "Basic secret".parse().unwrap());
        assert!(bearer(&headers).is_none());
        headers.insert(
            header::AUTHORIZATION,
            "bearer test-admin-token-value".parse().unwrap(),
        );
        assert_eq!(bearer(&headers).as_deref(), Some("test-admin-token-value"));
        assert_eq!(authenticate(&state, &headers).unwrap(), Scope::Admin);
        headers.insert(
            header::AUTHORIZATION,
            "Bearer missing-token-value".parse().unwrap(),
        );
        assert!(authenticate(&state, &headers).is_err());
    }

    #[tokio::test]
    async fn require_read_and_require_admin_and_the_routers_gate_the_api() {
        let (dir, mut state) = fixture();
        let static_dir = tempfile::tempdir().unwrap();
        std::fs::write(static_dir.path().join("index.html"), "<h1>catalog</h1>").unwrap();
        std::fs::create_dir_all(static_dir.path().join("nested")).unwrap();
        std::fs::write(
            static_dir.path().join("nested").join("index.html"),
            "nested",
        )
        .unwrap();
        state.static_dir = Some(static_dir.path().to_path_buf());

        let missing = call(
            router(state.clone(), &[]),
            "GET",
            "/api/v1/miniatures",
            None,
        )
        .await;
        assert_eq!(
            missing,
            StatusCode::UNAUTHORIZED,
            "require_read rejects a request with no token"
        );
        let (status, Json(_)) = create_key(
            State(state.clone()),
            JsonBody(KeyBody {
                name: "reader".into(),
                scope: "read".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        let Json(keys) = list_keys(State(state.clone())).await.unwrap();
        let read_id = keys["api_keys"][0]["id"].as_i64().unwrap();
        let read_token = {
            let (status, Json(created)) = create_key(
                State(state.clone()),
                JsonBody(KeyBody {
                    name: "second".into(),
                    scope: "read".into(),
                }),
            )
            .await
            .unwrap();
            assert_eq!(status, StatusCode::CREATED);
            let _ = read_id;
            created.token
        };
        let forbidden = call(
            router(state.clone(), &[]),
            "POST",
            "/api/v1/tags",
            Some(&read_token),
        )
        .await;
        assert_eq!(
            forbidden,
            StatusCode::FORBIDDEN,
            "require_admin rejects a read token"
        );
        let allowed = call(
            router(state.clone(), &[]),
            "GET",
            "/api/v1/tags",
            Some("test-admin-token-value"),
        )
        .await;
        assert_eq!(allowed, StatusCode::OK);

        let hidden = call(
            router(state.clone(), &[]),
            "GET",
            "/api/v1/desktop-session",
            None,
        )
        .await;
        assert_eq!(hidden, StatusCode::NOT_FOUND);
        let shown = call(
            ui_router(state.clone()),
            "GET",
            "/api/v1/desktop-session",
            None,
        )
        .await;
        assert_eq!(shown, StatusCode::OK);

        let origin = HeaderValue::from_static("http://127.0.0.1:5173");
        let cors = build(state.clone(), &[origin.clone()], false);
        let response = cors
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "http://127.0.0.1:5173"
        );

        let page = fallback(State(state.clone()), "/".parse().unwrap()).await;
        let bytes = page.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&bytes).unwrap().contains("catalog"));
        let nested = fallback(State(state.clone()), "/nested".parse().unwrap()).await;
        let bytes = nested.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..], b"nested");
        let missing = fallback(State(state.clone()), "/api/v1/nope".parse().unwrap()).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        let escaped = fallback(State(state.clone()), "/../secret".parse().unwrap()).await;
        assert_eq!(escaped.status(), StatusCode::NOT_FOUND);

        let doc = openapi_doc().await;
        assert_eq!(
            doc.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/yaml; charset=utf-8"
        );
        let bytes = doc.into_body().collect().await.unwrap().to_bytes();
        assert!(std::str::from_utf8(&bytes).unwrap().contains("openapi"));
        let Json(health) = super::health().await;
        assert_eq!(health["status"], "ok");
        assert!(dir.path().join("catalog.db").is_file());
    }
}

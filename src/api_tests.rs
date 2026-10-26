use crate::{router, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

struct TestApp {
    app: axum::Router,
    token: String,
    _dir: tempfile::TempDir,
}

fn test_app(seed: bool) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let token = "test-admin-token-value".to_string();
    let state = AppState::open(dir.path(), &token, None).unwrap();
    if seed {
        state.seed_example().unwrap();
    }
    let app = router(state, &[]);
    TestApp {
        app,
        token,
        _dir: dir,
    }
}

async fn call(
    app: &axum::Router,
    token: Option<&str>,
    method: &str,
    uri: &str,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = if let Some(body) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    if bytes.is_empty() {
        return (status, Value::Null);
    }
    let value =
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
    (status, value)
}

#[tokio::test]
async fn health_is_public_and_catalog_requires_a_token() {
    let test = test_app(false);
    let (status, body) = call(&test.app, None, "GET", "/health", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    let (status, body) = call(&test.app, None, "GET", "/api/v1/miniatures", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body["error"].as_str().unwrap().contains("token"));

    let (status, spec) = call(&test.app, None, "GET", "/openapi.yaml", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(spec
        .as_str()
        .unwrap_or("")
        .contains("Miniature Catalog API"));
}

#[tokio::test]
async fn read_key_can_prep_but_cannot_write_and_revoke_cuts_it_off() {
    let test = test_app(true);
    let (status, created) = call(
        &test.app,
        Some(&test.token),
        "POST",
        "/api/v1/api-keys",
        Some(r#"{"name":"encounter-generator","scope":"read"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let read_token = created["token"].as_str().unwrap().to_string();
    assert!(read_token.starts_with("mc_"));
    assert!(!created
        .to_string()
        .contains(&crate::auth::hash_token(&read_token)));

    let (status, _) = call(
        &test.app,
        Some(&read_token),
        "POST",
        "/api/v1/locations",
        Some(r#"{"name":"Shed","kind":"room"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, prep) = call(
        &test.app,
        Some(&read_token),
        "POST",
        "/api/v1/prep",
        Some(
            r#"{
                "requests": [
                    {"label":"orcs","races":["orc"],"quantity":5},
                    {"label":"more orcs","races":["orc"],"quantity":3},
                    {"label":"missing elves","races":["elf"],"roles":["wizard"],"quantity":2}
                ]
            }"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{prep}");

    let groups = prep["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[0]["location_path"],
        "Garage > Cabinet A > Drawer 1 > Tray A"
    );
    assert_eq!(groups[0]["picks"][0]["name"], "Orc Archer");
    assert_eq!(groups[0]["picks"][0]["quantity"], 2);
    assert_eq!(
        groups[1]["location_path"],
        "Garage > Cabinet A > Drawer 3 > Box 2"
    );
    assert_eq!(groups[1]["picks"][0]["name"], "Orc Warrior");
    assert_eq!(groups[1]["picks"][0]["quantity"], 3);
    assert_eq!(groups[1]["picks"][1]["name"], "Orc Warrior");
    assert_eq!(groups[1]["picks"][1]["quantity"], 1);
    assert_eq!(groups[1]["picks"][1]["request_index"], 1);

    assert_eq!(prep["requests"][0]["allocated"], 5);
    assert_eq!(prep["requests"][0]["short"], 0);
    assert_eq!(prep["requests"][1]["allocated"], 1);
    assert_eq!(prep["requests"][1]["short"], 2);
    assert_eq!(prep["requests"][2]["allocated"], 0);
    assert_eq!(prep["requests"][2]["short"], 2);

    let shortages = prep["shortages"].as_array().unwrap();
    assert_eq!(shortages.len(), 2);
    assert_eq!(shortages[0]["label"], "more orcs");
    assert_eq!(shortages[0]["available"], 1);
    assert_eq!(shortages[1]["label"], "missing elves");
    assert_eq!(shortages[1]["available"], 0);

    let (status, keys) = call(
        &test.app,
        Some(&test.token),
        "GET",
        "/api/v1/api-keys",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let key_id = keys["api_keys"][0]["id"].as_i64().unwrap();
    assert!(
        keys["api_keys"][0]["key_hash"].is_null() || keys["api_keys"][0].get("key_hash").is_none()
    );
    let (status, _) = call(
        &test.app,
        Some(&test.token),
        "DELETE",
        &format!("/api/v1/api-keys/{key_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &test.app,
        Some(&read_token),
        "GET",
        "/api/v1/miniatures",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn search_filters_location_path_and_report() {
    let test = test_app(true);
    let (status, locations) = call(
        &test.app,
        Some(&test.token),
        "GET",
        "/api/v1/locations",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cabinet = locations["locations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|location| location["name"] == "Cabinet A")
        .unwrap();
    assert_eq!(cabinet["path"], "Garage > Cabinet A");
    let cabinet_id = cabinet["id"].as_i64().unwrap();

    let (status, found) = call(
        &test.app,
        Some(&test.token),
        "GET",
        "/api/v1/miniatures?q=wiz&race=human&role=mage,sorcerer&size=32mm&game=Descent",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["miniatures"].as_array().unwrap().len(), 1);
    assert_eq!(found["miniatures"][0]["name"], "Human Wizard");
    assert_eq!(
        found["miniatures"][0]["location_path"],
        "Office > Shelf > Container 1"
    );
    assert!(found["miniatures"][0]["thumbnail_url"]
        .as_str()
        .unwrap()
        .ends_with("/thumbnail"));

    let (status, in_cabinet) = call(
        &test.app,
        Some(&test.token),
        "GET",
        &format!("/api/v1/miniatures?location_id={cabinet_id}&combat=melee"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = in_cabinet["miniatures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mini| mini["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["Goblin Spearman", "Orc Warrior", "Skeleton Warrior"]
    );

    let (status, report) = call(
        &test.app,
        Some(&test.token),
        "GET",
        "/api/v1/reports/locations",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let row = report["locations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "Cabinet A")
        .unwrap();
    assert_eq!(row["direct_quantity"], 0);
    assert_eq!(row["total_quantity"], 19);
}

#[tokio::test]
async fn prep_rejects_a_request_with_no_criteria() {
    let test = test_app(true);
    let (status, body) = call(
        &test.app,
        Some(&test.token),
        "POST",
        "/api/v1/prep",
        Some(r#"{"requests":[{"quantity":1}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("request 0"));
}

#[tokio::test]
async fn thumbnail_is_small_jpeg_and_original_is_not_in_the_database() {
    let test = test_app(false);
    let (status, location) = call(
        &test.app,
        Some(&test.token),
        "POST",
        "/api/v1/locations",
        Some(r#"{"name":"Desk","kind":"room"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let location_id = location["id"].as_i64().unwrap();
    let (status, mini) = call(
        &test.app,
        Some(&test.token),
        "POST",
        "/api/v1/miniatures",
        Some(&format!(
            r#"{{"name":"Knight","quantity":1,"size":"28mm","location_id":{location_id},"races":["human"],"combat":["melee"]}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{mini}");
    let id = mini["id"].as_i64().unwrap();

    let image = image::RgbImage::from_fn(900, 700, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 199) as u8, 20])
    });
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    assert!(png.len() > 10_000);

    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/miniatures/{id}/thumbnail"))
        .header("authorization", format!("Bearer {}", test.token))
        .header("content-type", "image/png")
        .body(Body::from(png.clone()))
        .unwrap();
    let response = test.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let request = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/miniatures/{id}/thumbnail"))
        .header("authorization", format!("Bearer {}", test.token))
        .body(Body::empty())
        .unwrap();
    let response = test.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert!(decoded.width() <= 256 && decoded.height() <= 256);
    assert!(decoded.width() < 900 && decoded.height() < 700);
    assert!(bytes.starts_with(&[0xFF, 0xD8]));

    let db_bytes = std::fs::read(test._dir.path().join("catalog.db")).unwrap();
    assert!(!db_bytes
        .windows(8)
        .any(|window| window == b"\x89PNG\r\n\x1a\n"));
}

#[tokio::test]
async fn drive_import_rejects_a_non_drive_link_without_fetching_it() {
    let test = test_app(false);
    let (status, location) = call(
        &test.app,
        Some(&test.token),
        "POST",
        "/api/v1/locations",
        Some(r#"{"name":"Desk","kind":"room"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let location_id = location["id"].as_i64().unwrap();
    let (status, mini) = call(
        &test.app,
        Some(&test.token),
        "POST",
        "/api/v1/miniatures",
        Some(&format!(
            r#"{{"name":"Knight","quantity":1,"size":"28mm","location_id":{location_id}}}"#
        )),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = mini["id"].as_i64().unwrap();
    let (status, body) = call(
        &test.app,
        Some(&test.token),
        "POST",
        &format!("/api/v1/miniatures/{id}/thumbnail/import"),
        Some(r#"{"source":"http://169.254.169.254/latest/meta-data"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("Drive"));
}

#[tokio::test]
async fn deleting_a_location_that_still_holds_miniatures_conflicts() {
    let test = test_app(true);
    let (status, locations) = call(
        &test.app,
        Some(&test.token),
        "GET",
        "/api/v1/locations",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let box2 = locations["locations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|location| location["name"] == "Box 2")
        .unwrap();
    let (status, body) = call(
        &test.app,
        Some(&test.token),
        "DELETE",
        &format!("/api/v1/locations/{}", box2["id"].as_i64().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("miniatures"));
}

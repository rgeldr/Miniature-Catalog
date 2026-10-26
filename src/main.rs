use miniature_catalog::{load_or_create_admin_token, resolve_data_dir, router, AppState};
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let data_dir = match resolve_data_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };
    let admin = match std::env::var("MC_ADMIN_TOKEN") {
        Ok(value) if value.trim().chars().count() >= 16 => {
            eprintln!("using MC_ADMIN_TOKEN from the environment");
            value
        }
        Ok(_) => {
            eprintln!("MC_ADMIN_TOKEN must be at least 16 characters");
            std::process::exit(1);
        }
        Err(_) => match load_or_create_admin_token(&data_dir) {
            Ok(token) => {
                println!("Admin token: {token}");
                println!("Saved at {}", data_dir.join("admin.token").display());
                token
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        },
    };
    let static_setting = std::env::var("MC_STATIC_DIR").unwrap_or_else(|_| "frontend/dist".into());
    let static_dir = PathBuf::from(&static_setting);
    let static_dir = static_dir.is_dir().then_some(static_dir);
    let origins = match parse_origins(&std::env::var("MC_CORS_ORIGINS").unwrap_or_default()) {
        Ok(origins) => origins,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let state = match AppState::open(&data_dir, &admin, static_dir) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("could not open database: {err:?}");
            std::process::exit(1);
        }
    };
    match state.seed_example() {
        Ok(true) => eprintln!("seeded the example collection"),
        Ok(false) => {}
        Err(err) => {
            eprintln!("could not seed database: {err:?}");
            std::process::exit(1);
        }
    }

    let bind = std::env::var("MC_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let addr: SocketAddr = match bind.parse() {
        Ok(addr) => addr,
        Err(_) => {
            eprintln!("MC_BIND must be an IP and port, for example 127.0.0.1:8080");
            std::process::exit(1);
        }
    };
    let app = router(state, &origins);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("could not listen on {addr}: {err}");
            std::process::exit(1);
        }
    };
    println!("Miniature Catalog API listening on http://{addr}");
    println!("Desktop window: cd frontend && npm run tauri dev");
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("server error: {err}");
        std::process::exit(1);
    }
}

fn parse_origins(raw: &str) -> Result<Vec<http::HeaderValue>, String> {
    let mut origins = Vec::new();
    for part in raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let value = http::HeaderValue::from_str(part)
            .map_err(|_| format!("MC_CORS_ORIGINS contains an invalid origin: {part}"))?;
        origins.push(value);
    }
    Ok(origins)
}

use miniature_catalog::{router, AppState};
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let admin = match std::env::var("MC_ADMIN_TOKEN") {
        Ok(value) if value.trim().chars().count() >= 16 => value,
        Ok(_) => {
            eprintln!("MC_ADMIN_TOKEN must be at least 16 characters");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!(
                "MC_ADMIN_TOKEN is required. Example: export MC_ADMIN_TOKEN=$(openssl rand -hex 24)"
            );
            std::process::exit(1);
        }
    };
    let data_dir = std::env::var("MC_DATA_DIR").unwrap_or_else(|_| "data".into());
    let bind = std::env::var("MC_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let static_setting = std::env::var("MC_STATIC_DIR").unwrap_or_else(|_| "frontend/dist".into());
    let static_dir = PathBuf::from(&static_setting);
    let static_dir = static_dir.is_dir().then_some(static_dir);
    if static_dir.is_none() {
        eprintln!("no UI directory at {static_setting}; API only");
    }
    let origins = match parse_origins(&std::env::var("MC_CORS_ORIGINS").unwrap_or_default()) {
        Ok(origins) => origins,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let state = match AppState::open(std::path::Path::new(&data_dir), &admin, static_dir) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("could not open database: {err:?}");
            std::process::exit(1);
        }
    };
    if std::env::var("MC_SEED").ok().as_deref() == Some("1") {
        match state.seed_example() {
            Ok(true) => eprintln!("seeded the example collection"),
            Ok(false) => eprintln!("database already has data; left it unchanged"),
            Err(err) => {
                eprintln!("could not seed database: {err:?}");
                std::process::exit(1);
            }
        }
    }

    let app = router(state, &origins);
    let addr: SocketAddr = match bind.parse() {
        Ok(addr) => addr,
        Err(_) => {
            eprintln!("MC_BIND must be host:port, for example 127.0.0.1:8080");
            std::process::exit(1);
        }
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("could not listen on {addr}: {err}");
            std::process::exit(1);
        }
    };
    println!("Miniature Catalog listening on http://{addr}");
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

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use miniature_catalog::{
    listen_for_ui, load_or_create_admin_token, load_settings, resolve_data_dir, ApiHost, AppState,
    Settings,
};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let port = launch(app).map_err(box_err)?;
            eprintln!("catalog window: http://127.0.0.1:{port}");
            let url = format!("http://127.0.0.1:{port}")
                .parse()
                .map_err(|err: url::ParseError| box_err(err.to_string()))?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Miniature Catalog")
                .inner_size(1100.0, 800.0)
                .min_inner_size(720.0, 520.0)
                .build()
                .map_err(|err| box_err(err.to_string()))?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Miniature Catalog");
}

fn box_err(message: String) -> Box<dyn std::error::Error> {
    Box::new(std::io::Error::other(message))
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

fn launch(app: &tauri::App) -> Result<u16, String> {
    let static_dir = resolve_static(app)?;
    let data_dir = resolve_data_dir()?;
    let token = load_or_create_admin_token(&data_dir)?;
    let mut state = AppState::open(&data_dir, &token, Some(static_dir))
        .map_err(|err| format!("could not open the catalog database: {err:?}"))?;
    state.host = Some(ApiHost::new());
    match state.seed_example() {
        Ok(true) => eprintln!("seeded the example collection"),
        Ok(false) => {}
        Err(err) => return Err(format!("could not seed the catalog: {err:?}")),
    }
    eprintln!("catalog data: {}", state.data_dir.display());
    let state_for_api = state.clone();
    runtime().block_on(async move {
        let settings = load_settings(&state_for_api.data_dir).unwrap_or_else(|err| {
            eprintln!("{err}");
            Settings::default()
        });
        if let Some(host) = &state_for_api.host {
            if let Err(err) = host.apply(state_for_api.clone(), &settings).await {
                eprintln!("encounter API was not started: {err}");
            }
        }
        listen_for_ui(state_for_api).await
    })
}

fn resolve_static(app: &tauri::App) -> Result<PathBuf, String> {
    if let Ok(value) = std::env::var("MC_STATIC_DIR") {
        let path = PathBuf::from(&value);
        return if path.join("index.html").is_file() {
            Ok(path)
        } else {
            Err(format!("MC_STATIC_DIR has no index.html: {value}"))
        };
    }
    let mut candidates = Vec::new();
    // Vite writes frontend/dist. While developing, that directory is newer than
    // the copy Tauri places next to the debug binary, and `tauri dev` runs
    // with src-tauri as the working directory.
    candidates.push(PathBuf::from("../dist"));
    candidates.push(PathBuf::from("frontend/dist"));
    if let Ok(resource) = app.path().resource_dir() {
        candidates.push(resource.join("dist"));
        candidates.push(resource);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("dist"));
        }
    }
    candidates.push(PathBuf::from("dist"));
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
        .ok_or_else(|| {
            "could not find the catalog UI. From the frontend folder run npm run build.".into()
        })
}

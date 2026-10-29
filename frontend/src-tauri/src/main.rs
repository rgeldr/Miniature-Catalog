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

fn launch<R: tauri::Runtime>(app: &tauri::App<R>) -> Result<u16, String> {
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

fn resolve_static<R: tauri::Runtime>(app: &tauri::App<R>) -> Result<PathBuf, String> {
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

#[cfg(test)]
mod exit_capability {
    use std::path::PathBuf;
    use tauri::utils::acl::capability::CapabilityFile;
    use tauri::utils::acl::RemoteUrlPattern;

    #[test]
    fn exit_may_close_only_the_local_catalog_origin() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("capabilities/default.json");
        let CapabilityFile::Capability(capability) =
            CapabilityFile::load(&path).expect("capability file")
        else {
            panic!("expected one capability");
        };
        assert!(
            capability
                .permissions
                .iter()
                .any(|entry| entry.identifier().get() == "core:window:allow-close"),
            "Exit calls plugin:window|close, which is not in the default window permissions"
        );
        let urls = capability
            .remote
            .as_ref()
            .expect("the catalog page is a remote URL")
            .urls
            .clone();
        assert_eq!(urls, vec!["http://127.0.0.1:*/*".to_string()]);
        let pattern: RemoteUrlPattern = urls[0].parse().expect("url pattern");
        assert!(pattern.test(&"http://127.0.0.1:8094/".parse().unwrap()));
        assert!(pattern.test(&"http://127.0.0.1:1420/index.html".parse().unwrap()));
        assert!(!pattern.test(&"http://example.com/".parse().unwrap()));
        assert!(!pattern.test(&"https://127.0.0.1:8094/".parse().unwrap()));
    }
}

#[cfg(test)]
mod shell_tests {
    use super::{box_err, launch, resolve_static, runtime};
    use std::sync::Mutex;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn set_var(key: &str, value: Option<&std::path::Path>) -> Option<String> {
        let previous = std::env::var(key).ok();
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        previous
    }

    fn restore(key: &str, previous: Option<String>) {
        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    #[test]
    fn box_err_and_runtime_are_stable() {
        let err = box_err("no window".into());
        assert!(err.to_string().contains("no window"));
        assert!(std::ptr::eq(runtime(), runtime()));
    }

    #[test]
    fn resolve_static_requires_index_html_in_mc_static_dir() {
        let _lock = env_lock();
        let root = std::env::temp_dir().join(format!(
            "mc-static-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ready = root.join("ready");
        let empty = root.join("empty");
        std::fs::create_dir_all(&ready).unwrap();
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(ready.join("index.html"), "<!doctype html>").unwrap();
        let previous = set_var("MC_STATIC_DIR", Some(&ready));
        let app = tauri::test::mock_app();
        let found = resolve_static(&app).unwrap();
        assert_eq!(found, ready);
        set_var("MC_STATIC_DIR", Some(&empty));
        let err = resolve_static(&app).unwrap_err();
        assert!(err.contains("index.html"), "{err}");
        restore("MC_STATIC_DIR", previous);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn launch_serves_the_catalog_from_the_override_directories() {
        let _lock = env_lock();
        let root = std::env::temp_dir().join(format!(
            "mc-shell-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let static_dir = root.join("static");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&static_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::write(
            static_dir.join("index.html"),
            "<!doctype html><title>catalog</title>",
        )
        .unwrap();
        let previous_static = set_var("MC_STATIC_DIR", Some(&static_dir));
        let previous_data = set_var("MC_DATA_DIR", Some(&data_dir));
        let previous_token = {
            let previous = std::env::var("MC_ADMIN_TOKEN").ok();
            unsafe { std::env::remove_var("MC_ADMIN_TOKEN") };
            previous
        };
        let app = tauri::test::mock_app();
        let port = launch(&app).expect("launch");
        let mut body = String::new();
        for _ in 0..50 {
            if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                use std::io::{Read, Write};
                let _ = stream.write_all(
                    b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                );
                let _ = stream.read_to_string(&mut body);
                if body.contains("ok") {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(body.contains("ok"), "{body}");
        assert!(data_dir.join("catalog.db").is_file());
        restore("MC_STATIC_DIR", previous_static);
        restore("MC_DATA_DIR", previous_data);
        restore("MC_ADMIN_TOKEN", previous_token);
        let _ = std::fs::remove_dir_all(root);
    }
}

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};

pub const DEFAULT_API_BIND: &str = "127.0.0.1:8080";
const TOKEN_FILE: &str = "admin.token";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub api_enabled: bool,
    pub api_bind: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_enabled: false,
            api_bind: DEFAULT_API_BIND.to_string(),
        }
    }
}

/// Encounter-generator listener. The catalog window uses a different port, so
/// stopping this one does not close the window.
pub struct ApiHost {
    inner: Mutex<Option<Running>>,
}

struct Running {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    stopped: Option<oneshot::Receiver<()>>,
}

impl ApiHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(None),
        })
    }

    pub async fn bound_addr(&self) -> Option<SocketAddr> {
        self.inner.lock().await.as_ref().map(|running| running.addr)
    }

    /// Replace the listener. Bind failures leave the previous listener stopped
    /// only until the caller restores it.
    pub async fn apply(&self, state: AppState, settings: &Settings) -> Result<(), String> {
        if settings.api_enabled {
            validate_api_bind(&settings.api_bind)?;
        }
        self.stop().await;
        if !settings.api_enabled {
            return Ok(());
        }
        let addr = validate_api_bind(&settings.api_bind)?;
        let listener = bind_with_retry(addr).await?;
        let bound = listener
            .local_addr()
            .map_err(|err| format!("could not read the API address: {err}"))?;
        let mut api_state = state;
        api_state.static_dir = None;
        let app = crate::routes::router(api_state, &[]);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (stopped_tx, stopped_rx) = oneshot::channel();
        tokio::spawn(async move {
            let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            });
            if let Err(err) = server.await {
                eprintln!("api server error: {err}");
            }
            let _ = stopped_tx.send(());
        });
        *self.inner.lock().await = Some(Running {
            addr: bound,
            shutdown: Some(shutdown_tx),
            stopped: Some(stopped_rx),
        });
        Ok(())
    }

    pub async fn stop(&self) {
        let running = self.inner.lock().await.take();
        if let Some(Running {
            shutdown, stopped, ..
        }) = running
        {
            if let Some(tx) = shutdown {
                let _ = tx.send(());
            }
            if let Some(rx) = stopped {
                let _ = tokio::time::timeout(Duration::from_secs(2), rx).await;
            }
        }
    }
}

pub fn app_data_dir() -> Result<PathBuf, String> {
    directories::ProjectDirs::from(
        "com.miniaturecatalog",
        "MiniatureCatalog",
        "MiniatureCatalog",
    )
    .map(|dirs| dirs.data_dir().to_path_buf())
    .ok_or_else(|| "could not find the user data directory".to_string())
}

pub fn resolve_data_dir() -> Result<PathBuf, String> {
    if let Ok(value) = std::env::var("MC_DATA_DIR") {
        if !value.trim().is_empty() {
            return Ok(PathBuf::from(value));
        }
    }
    app_data_dir()
}

pub fn load_or_create_admin_token(data_dir: &Path) -> Result<String, String> {
    if let Some(token) = env_admin_token()? {
        return Ok(token);
    }
    load_or_create_saved_token(data_dir)
}

pub fn load_settings(data_dir: &Path) -> Result<Settings, String> {
    let path = data_dir.join(SETTINGS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|err| format!("settings.json is invalid: {err}"))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(err) => Err(format!("read settings: {err}")),
    }
}

pub fn save_settings(data_dir: &Path, settings: &Settings) -> Result<(), String> {
    validate_api_bind(&settings.api_bind)?;
    std::fs::create_dir_all(data_dir).map_err(|err| format!("create data dir: {err}"))?;
    let path = data_dir.join(SETTINGS_FILE);
    let text =
        serde_json::to_string_pretty(settings).map_err(|err| format!("encode settings: {err}"))?;
    std::fs::write(&path, format!("{text}\n")).map_err(|err| format!("write settings: {err}"))
}

pub fn validate_api_bind(value: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = value
        .parse()
        .map_err(|_| "Listen address must be an IP and port, such as 127.0.0.1:8080".to_string())?;
    if addr.port() == 0 {
        return Err("Choose a fixed port, such as 8080".to_string());
    }
    Ok(addr)
}

pub fn spawn_ui(listener: tokio::net::TcpListener, state: AppState) {
    let app = crate::routes::ui_router(state);
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("catalog window server error: {err}");
        }
    });
}

pub async fn listen_for_ui(state: AppState) -> Result<u16, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| format!("could not bind the window server: {err}"))?;
    let port = listener
        .local_addr()
        .map_err(|err| format!("could not read the window port: {err}"))?
        .port();
    spawn_ui(listener, state);
    Ok(port)
}

fn env_admin_token() -> Result<Option<String>, String> {
    match std::env::var("MC_ADMIN_TOKEN") {
        Ok(value) => {
            let value = value.trim().to_string();
            if value.chars().count() < 16 {
                return Err("MC_ADMIN_TOKEN must be at least 16 characters".to_string());
            }
            Ok(Some(value))
        }
        Err(_) => Ok(None),
    }
}

pub fn load_or_create_saved_token(data_dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(data_dir).map_err(|err| format!("create data dir: {err}"))?;
    let path = data_dir.join(TOKEN_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let text = text.trim().to_string();
            if text.chars().count() < 16 {
                return Err(
                    "saved admin token is unusable; delete admin.token and start again".to_string(),
                );
            }
            Ok(text)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let token = crate::auth::generate_token();
            write_secret(&path, &format!("{token}\n"))?;
            Ok(token)
        }
        Err(err) => Err(format!("read admin token: {err}")),
    }
}

fn write_secret(path: &Path, contents: &str) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|err| format!("write {}: {err}", path.display()))?;
    file.write_all(contents.as_bytes())
        .map_err(|err| format!("write {}: {err}", path.display()))?;
    Ok(())
}

async fn bind_with_retry(addr: SocketAddr) -> Result<tokio::net::TcpListener, String> {
    let mut last = None;
    for _ in 0..40 {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(err) => {
                last = Some(err);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    Err(format!(
        "could not listen on {addr}: {}",
        last.expect("bind was retried")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use serde_json::Value;

    #[test]
    fn saved_admin_token_is_created_once_and_kept_private() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create_saved_token(dir.path()).unwrap();
        let second = load_or_create_saved_token(dir.path()).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("mc_"));
        assert!(first.chars().count() > 16);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(TOKEN_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn settings_default_to_off_and_localhost() {
        let dir = tempfile::tempdir().unwrap();
        let settings = load_settings(dir.path()).unwrap();
        assert!(!settings.api_enabled);
        assert_eq!(settings.api_bind, DEFAULT_API_BIND);
        let updated = Settings {
            api_enabled: true,
            api_bind: "127.0.0.1:9090".into(),
        };
        save_settings(dir.path(), &updated).unwrap();
        assert_eq!(load_settings(dir.path()).unwrap(), updated);
    }

    #[test]
    fn api_bind_requires_an_ip_and_a_fixed_port() {
        assert!(validate_api_bind("127.0.0.1:8080").is_ok());
        assert!(validate_api_bind("[::1]:8080").is_ok());
        assert!(validate_api_bind("0.0.0.0:8080").is_ok());
        assert!(validate_api_bind("localhost:8080").is_err());
        assert!(validate_api_bind("127.0.0.1").is_err());
        assert!(validate_api_bind("127.0.0.1:0").is_err());
        let dir = tempfile::tempdir().unwrap();
        let bad = Settings {
            api_enabled: true,
            api_bind: "localhost:8080".into(),
        };
        assert!(save_settings(dir.path(), &bad).is_err());
    }

    #[test]
    fn app_data_dir_names_this_catalog() {
        let dir = app_data_dir().unwrap();
        let text = dir.to_string_lossy().to_ascii_lowercase();
        assert!(
            text.contains("miniaturecatalog"),
            "unexpected data dir {dir:?}"
        );
    }

    #[tokio::test]
    async fn api_toggle_opens_a_real_socket_and_hides_the_window_token() {
        let dir = tempfile::tempdir().unwrap();
        let token = "test-admin-token-value";
        let mut state = AppState::open(dir.path(), token, None).unwrap();
        let host = ApiHost::new();
        state.host = Some(host.clone());

        let port = listen_for_ui(state.clone()).await.unwrap();
        let ui_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        let client = state.http.clone();
        let session: Value = client
            .get(format!("http://{ui_addr}/api/v1/desktop-session"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(session["token"], token);

        let port = free_port().await;
        let bind = format!("127.0.0.1:{port}");
        let health = format!("http://{bind}/health");
        assert!(client.get(&health).send().await.is_err());

        let started = put_settings(&client, ui_addr, token, true, &bind).await;
        assert_eq!(started["listening"], true);
        assert_eq!(started["bound"], bind.as_str());
        let public_health = client.get(&health).send().await.unwrap();
        assert!(public_health.status().is_success());

        let leaked = client
            .get(format!("http://{bind}/api/v1/desktop-session"))
            .send()
            .await
            .unwrap();
        assert_eq!(leaked.status(), reqwest::StatusCode::NOT_FOUND);

        let kept = put_settings(&client, ui_addr, token, true, "localhost:8080").await;
        assert_eq!(
            kept["error"],
            "Listen address must be an IP and port, such as 127.0.0.1:8080"
        );
        assert!(client
            .get(&health)
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
        assert_eq!(load_settings(dir.path()).unwrap().api_bind, bind);

        let stopped = put_settings(&client, ui_addr, token, false, &bind).await;
        assert_eq!(stopped["listening"], false);
        assert!(!load_settings(dir.path()).unwrap().api_enabled);
        wait_until_down(&client, &health).await;
    }

    fn with_env(key: &str, value: Option<&str>, check: impl FnOnce()) {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|err| err.into_inner());
        let previous = std::env::var(key).ok();
        match value {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(check));
        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }

    #[test]
    fn resolve_data_dir_and_env_admin_token_honor_the_process_environment() {
        let dir = tempfile::tempdir().unwrap();
        with_env("MC_DATA_DIR", Some(dir.path().to_str().unwrap()), || {
            assert_eq!(resolve_data_dir().unwrap(), dir.path());
        });
        with_env("MC_DATA_DIR", Some("   "), || {
            let fallback = resolve_data_dir().unwrap();
            assert!(fallback
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("miniaturecatalog"));
        });
        with_env("MC_ADMIN_TOKEN", None, || {
            assert!(env_admin_token().unwrap().is_none());
        });
        with_env("MC_ADMIN_TOKEN", Some("too-short"), || {
            assert!(env_admin_token().unwrap_err().contains("16"));
        });
        with_env("MC_ADMIN_TOKEN", Some("  env-admin-token-value  "), || {
            assert_eq!(
                env_admin_token().unwrap().as_deref(),
                Some("env-admin-token-value")
            );
            assert_eq!(
                load_or_create_admin_token(dir.path()).unwrap(),
                "env-admin-token-value"
            );
        });
        let settings = Settings::default();
        assert!(!settings.api_enabled);
        assert_eq!(settings.api_bind, DEFAULT_API_BIND);
        let secret = dir.path().join("secret.txt");
        write_secret(&secret, "token\n").unwrap();
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "token\n");
    }

    #[tokio::test]
    async fn apply_stop_spawn_ui_and_bind_with_retry_open_real_sockets() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::open(dir.path(), "test-admin-token-value", None).unwrap();
        let host = ApiHost::new();
        assert!(host.bound_addr().await.is_none());
        let probe = bind_with_retry("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let settings = Settings {
            api_enabled: true,
            api_bind: format!("127.0.0.1:{port}"),
        };
        host.apply(state.clone(), &settings).await.unwrap();
        assert_eq!(host.bound_addr().await.unwrap().port(), port);
        let health = reqwest::get(format!("http://127.0.0.1:{port}/health"))
            .await
            .unwrap();
        assert!(health.status().is_success());
        host.stop().await;
        assert!(host.bound_addr().await.is_none());
        host.apply(state.clone(), &Settings::default())
            .await
            .unwrap();
        assert!(host.bound_addr().await.is_none());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ui_port = listener.local_addr().unwrap().port();
        spawn_ui(listener, state);
        let mut session = None;
        for _ in 0..50 {
            if let Ok(response) =
                reqwest::get(format!("http://127.0.0.1:{ui_port}/api/v1/desktop-session")).await
            {
                session = Some(response.json::<Value>().await.unwrap());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(session.unwrap()["token"], "test-admin-token-value");
    }

    async fn free_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    async fn put_settings(
        client: &reqwest::Client,
        ui_addr: SocketAddr,
        token: &str,
        enabled: bool,
        bind: &str,
    ) -> Value {
        let response = client
            .put(format!("http://{ui_addr}/api/v1/settings"))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(format!(
                r#"{{"api_enabled":{enabled},"api_bind":"{bind}"}}"#
            ))
            .send()
            .await
            .unwrap();
        response.json().await.unwrap()
    }

    async fn wait_until_down(client: &reqwest::Client, url: &str) {
        for _ in 0..50 {
            if client.get(url).send().await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("api still listening at {url}");
    }
}

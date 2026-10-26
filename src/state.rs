use crate::auth;
use crate::db;
use crate::error::{ApiError, DbError};
use crate::thumb;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    pub admin_hash: String,
    pub thumbs_dir: PathBuf,
    pub static_dir: Option<PathBuf>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn open(
        data_dir: &Path,
        admin_token: &str,
        static_dir: Option<PathBuf>,
    ) -> Result<Self, DbError> {
        let conn = db::open_connection(&data_dir.join("catalog.db"))?;
        let thumbs_dir = data_dir.join("thumbs");
        std::fs::create_dir_all(&thumbs_dir)
            .map_err(|err| DbError::Internal(format!("create thumbs dir: {err}")))?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|err| DbError::Internal(format!("http client: {err}")))?;
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            admin_hash: auth::hash_token(admin_token),
            thumbs_dir,
            static_dir,
            http,
        })
    }

    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>, ApiError> {
        self.db.lock().map_err(|_| {
            eprintln!("database lock poisoned");
            ApiError::internal()
        })
    }

    pub fn seed_example(&self) -> Result<bool, DbError> {
        let seeded = {
            let conn = self
                .db
                .lock()
                .map_err(|_| DbError::Internal("database lock poisoned".into()))?;
            db::seed_example(&conn)?
        };
        if !seeded {
            return Ok(false);
        }
        let miniatures = {
            let conn = self
                .db
                .lock()
                .map_err(|_| DbError::Internal("database lock poisoned".into()))?;
            db::list_miniatures(&conn, &db::MiniFilter::default())?
        };
        for mini in miniatures {
            let file = format!("{}.jpg", mini.id);
            let jpeg = thumb::solid_jpeg(thumb::seed_color(&mini.name));
            std::fs::write(self.thumbs_dir.join(&file), jpeg)
                .map_err(|err| DbError::Internal(format!("write thumb: {err}")))?;
            let conn = self
                .db
                .lock()
                .map_err(|_| DbError::Internal("database lock poisoned".into()))?;
            db::set_thumbnail(&conn, mini.id, &file)?;
        }
        Ok(true)
    }
}

use std::{collections::HashMap, fs};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use clap::Parser;
use serde::Deserialize;

// -- Config

#[derive(Debug, Deserialize)]
struct ServerConfig {
    config: Config,
    storage: Storage,
}

#[derive(Debug, Deserialize)]
struct Config {
    bind: Option<String>,
    backend: String,
    auth: String,
}

#[derive(Debug, Deserialize)]
struct Storage {
    valkey: Option<String>,
    postgres: Option<String>,
    sqlite: Option<String>,
}

fn load_config(path: &str) -> anyhow::Result<ServerConfig> {
    let text = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&text)?)
}

// -- Backend

#[derive(Clone)]
enum Db {
    Valkey(redis::aio::MultiplexedConnection),
    Postgres(sqlx::PgPool),
    Sqlite(sqlx::SqlitePool),
}

impl Db {
    async fn connect(backend: &str, storage: &Storage) -> anyhow::Result<Self> {
        match backend {
            "valkey" => {
                let url = storage.valkey.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("backend is 'valkey' but no 'valkey:' URL configured")
                })?;
                let client = redis::Client::open(url.as_str())?;
                let conn = client.get_multiplexed_async_connection().await?;
                println!("✓ valkey connected");
                Ok(Db::Valkey(conn))
            }
            "postgres" => {
                let url = storage.postgres.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("backend is 'postgres' but no 'postgres:' URL configured")
                })?;
                let pool = sqlx::PgPool::connect(url).await?;
                Self::migrate_pg(&pool).await?;
                println!("✓ postgres connected");
                Ok(Db::Postgres(pool))
            }
            "sqlite" => {
                let path = storage.sqlite.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("backend is 'sqlite' but no 'sqlite:' path configured")
                })?;
                // sqlx sqlite needs the file to exist; create it if missing
                let file_path = path
                    .strip_prefix("sqlite://")
                    .or_else(|| path.strip_prefix("sqlite:"))
                    .unwrap_or(path);
                if !std::path::Path::new(file_path).exists() {
                    std::fs::File::create(file_path)?;
                }
                let pool = sqlx::SqlitePool::connect(path).await?;
                Self::migrate_sqlite(&pool).await?;
                println!("✓ sqlite connected ({file_path})");
                Ok(Db::Sqlite(pool))
            }
            other => {
                anyhow::bail!("unknown backend '{other}'. use 'valkey', 'postgres', or 'sqlite'")
            }
        }
    }

    // -- Valkey helpers --

    async fn v_projects(&mut self) -> anyhow::Result<Vec<String>> {
        let Db::Valkey(c) = self else { unreachable!() };
        use redis::AsyncCommands;
        Ok(c.smembers("envd:projects").await.unwrap_or_default())
    }

    async fn v_create_project(&mut self, name: &str) -> anyhow::Result<()> {
        let Db::Valkey(c) = self else { unreachable!() };
        use redis::AsyncCommands;
        let _: i64 = c.sadd("envd:projects", name).await?;
        Ok(())
    }

    async fn v_delete_project(&mut self, name: &str) -> anyhow::Result<()> {
        let Db::Valkey(c) = self else { unreachable!() };
        use redis::AsyncCommands;
        let _: i64 = c.srem("envd:projects", name).await?;
        let _: i64 = c.del(format!("envd:envs:{name}")).await?;
        Ok(())
    }

    async fn v_get_envs(&mut self, name: &str) -> anyhow::Result<HashMap<String, String>> {
        let Db::Valkey(c) = self else { unreachable!() };
        use redis::AsyncCommands;
        Ok(c.hgetall(format!("envd:envs:{name}"))
            .await
            .unwrap_or_default())
    }

    async fn v_set_envs(
        &mut self,
        name: &str,
        envs: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let Db::Valkey(c) = self else { unreachable!() };
        use redis::AsyncCommands;
        let _: i64 = c.sadd("envd:projects", name).await?;
        for (k, v) in envs {
            let _: i64 = c.hset(format!("envd:envs:{name}"), k, v).await?;
        }
        Ok(())
    }

    async fn v_delete_env(&mut self, name: &str, key: &str) -> anyhow::Result<()> {
        let Db::Valkey(c) = self else { unreachable!() };
        use redis::AsyncCommands;
        let _: i64 = c.hdel(format!("envd:envs:{name}"), key).await?;
        Ok(())
    }

    // -- Postgres helpers --

    async fn migrate_pg(pool: &sqlx::PgPool) -> anyhow::Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS projects (
                id      SERIAL PRIMARY KEY,
                name    TEXT UNIQUE NOT NULL,
                created TIMESTAMPTZ DEFAULT now()
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS envs (
                id      SERIAL PRIMARY KEY,
                project TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
                key     TEXT NOT NULL,
                value   TEXT NOT NULL,
                updated TIMESTAMPTZ DEFAULT now(),
                UNIQUE(project, key)
            )",
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn pg_projects(&self) -> anyhow::Result<Vec<String>> {
        let Db::Postgres(pool) = self else {
            unreachable!()
        };
        let rows: Vec<String> = sqlx::query_scalar("SELECT name FROM projects ORDER BY name")
            .fetch_all(pool)
            .await?;
        Ok(rows)
    }

    async fn pg_create_project(&self, name: &str) -> anyhow::Result<()> {
        let Db::Postgres(pool) = self else {
            unreachable!()
        };
        sqlx::query("INSERT INTO projects (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(())
    }

    async fn pg_delete_project(&self, name: &str) -> anyhow::Result<()> {
        let Db::Postgres(pool) = self else {
            unreachable!()
        };
        sqlx::query("DELETE FROM envs WHERE project = $1")
            .bind(name)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(name)
            .execute(pool)
            .await
            .ok();
        Ok(())
    }

    async fn pg_get_envs(&self, name: &str) -> anyhow::Result<HashMap<String, String>> {
        let Db::Postgres(pool) = self else {
            unreachable!()
        };
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM envs WHERE project = $1 ORDER BY key")
                .bind(name)
                .fetch_all(pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    async fn pg_set_envs(&self, name: &str, envs: &HashMap<String, String>) -> anyhow::Result<()> {
        let Db::Postgres(pool) = self else {
            unreachable!()
        };
        sqlx::query("INSERT INTO projects (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(name)
            .execute(pool)
            .await
            .ok();
        for (k, v) in envs {
            sqlx::query(
                "INSERT INTO envs (project, key, value) VALUES ($1,$2,$3)
                 ON CONFLICT (project, key) DO UPDATE SET value = $3, updated = now()",
            )
            .bind(name)
            .bind(k)
            .bind(v)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    async fn pg_delete_env(&self, name: &str, key: &str) -> anyhow::Result<()> {
        let Db::Postgres(pool) = self else {
            unreachable!()
        };
        sqlx::query("DELETE FROM envs WHERE project = $1 AND key = $2")
            .bind(name)
            .bind(key)
            .execute(pool)
            .await
            .ok();
        Ok(())
    }

    // -- SQLite helpers --

    async fn migrate_sqlite(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS projects (
                id      INTEGER PRIMARY KEY AUTOINCREMENT,
                name    TEXT UNIQUE NOT NULL,
                created TEXT DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS envs (
                id      INTEGER PRIMARY KEY AUTOINCREMENT,
                project TEXT NOT NULL REFERENCES projects(name) ON DELETE CASCADE,
                key     TEXT NOT NULL,
                value   TEXT NOT NULL,
                updated TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(project, key)
            )",
        )
        .execute(pool)
        .await?;

        // enable foreign keys
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(pool)
            .await?;
        Ok(())
    }

    async fn sq_projects(&self) -> anyhow::Result<Vec<String>> {
        let Db::Sqlite(pool) = self else {
            unreachable!()
        };
        let rows: Vec<String> = sqlx::query_scalar("SELECT name FROM projects ORDER BY name")
            .fetch_all(pool)
            .await?;
        Ok(rows)
    }

    async fn sq_create_project(&self, name: &str) -> anyhow::Result<()> {
        let Db::Sqlite(pool) = self else {
            unreachable!()
        };
        sqlx::query("INSERT INTO projects (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(())
    }

    async fn sq_delete_project(&self, name: &str) -> anyhow::Result<()> {
        let Db::Sqlite(pool) = self else {
            unreachable!()
        };
        sqlx::query("DELETE FROM envs WHERE project = $1")
            .bind(name)
            .execute(pool)
            .await
            .ok();
        sqlx::query("DELETE FROM projects WHERE name = $1")
            .bind(name)
            .execute(pool)
            .await
            .ok();
        Ok(())
    }

    async fn sq_get_envs(&self, name: &str) -> anyhow::Result<HashMap<String, String>> {
        let Db::Sqlite(pool) = self else {
            unreachable!()
        };
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM envs WHERE project = $1 ORDER BY key")
                .bind(name)
                .fetch_all(pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    async fn sq_set_envs(&self, name: &str, envs: &HashMap<String, String>) -> anyhow::Result<()> {
        let Db::Sqlite(pool) = self else {
            unreachable!()
        };
        sqlx::query("INSERT INTO projects (name) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(name)
            .execute(pool)
            .await
            .ok();
        for (k, v) in envs {
            sqlx::query(
                "INSERT INTO envs (project, key, value) VALUES ($1,$2,$3)
                 ON CONFLICT (project, key) DO UPDATE SET value = $3, updated = CURRENT_TIMESTAMP",
            )
            .bind(name)
            .bind(k)
            .bind(v)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    async fn sq_delete_env(&self, name: &str, key: &str) -> anyhow::Result<()> {
        let Db::Sqlite(pool) = self else {
            unreachable!()
        };
        sqlx::query("DELETE FROM envs WHERE project = $1 AND key = $2")
            .bind(name)
            .bind(key)
            .execute(pool)
            .await
            .ok();
        Ok(())
    }

    // -- Dispatch --

    async fn list_projects(&mut self) -> anyhow::Result<Vec<String>> {
        match self {
            Db::Valkey(..) => self.v_projects().await,
            Db::Postgres(..) => self.pg_projects().await,
            Db::Sqlite(..) => self.sq_projects().await,
        }
    }

    async fn create_project(&mut self, name: &str) -> anyhow::Result<()> {
        match self {
            Db::Valkey(..) => self.v_create_project(name).await,
            Db::Postgres(..) => self.pg_create_project(name).await,
            Db::Sqlite(..) => self.sq_create_project(name).await,
        }
    }

    async fn delete_project(&mut self, name: &str) -> anyhow::Result<()> {
        match self {
            Db::Valkey(..) => self.v_delete_project(name).await,
            Db::Postgres(..) => self.pg_delete_project(name).await,
            Db::Sqlite(..) => self.sq_delete_project(name).await,
        }
    }

    async fn get_envs(&mut self, name: &str) -> anyhow::Result<HashMap<String, String>> {
        match self {
            Db::Valkey(..) => self.v_get_envs(name).await,
            Db::Postgres(..) => self.pg_get_envs(name).await,
            Db::Sqlite(..) => self.sq_get_envs(name).await,
        }
    }

    async fn set_envs(&mut self, name: &str, envs: &HashMap<String, String>) -> anyhow::Result<()> {
        match self {
            Db::Valkey(..) => self.v_set_envs(name, envs).await,
            Db::Postgres(..) => self.pg_set_envs(name, envs).await,
            Db::Sqlite(..) => self.sq_set_envs(name, envs).await,
        }
    }

    async fn delete_env(&mut self, name: &str, key: &str) -> anyhow::Result<()> {
        match self {
            Db::Valkey(..) => self.v_delete_env(name, key).await,
            Db::Postgres(..) => self.pg_delete_env(name, key).await,
            Db::Sqlite(..) => self.sq_delete_env(name, key).await,
        }
    }
}

// -- State

#[derive(Clone)]
struct AppState {
    db: Db,
    auth: String,
}

// -- Auth

fn auth(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|t| t == state.auth)
        .unwrap_or(false)
}

macro_rules! require_auth {
    ($state:expr, $headers:expr) => {
        if !auth(&$state, &$headers) {
            return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
        }
    };
}

// -- Models

#[derive(Deserialize)]
struct SetEnvsBody {
    envs: HashMap<String, String>,
}

// -- Handlers

/// GET /projects
async fn list_projects(State(mut s): State<AppState>, headers: HeaderMap) -> Response {
    require_auth!(s, headers);
    match s.db.list_projects().await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /projects  body: { "name": "myapp" }
async fn create_project(
    State(mut s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    require_auth!(s, headers);
    let name = match body["name"].as_str() {
        Some(n) => n.to_string(),
        None => return (StatusCode::BAD_REQUEST, "missing name").into_response(),
    };
    match s.db.create_project(&name).await {
        Ok(_) => (StatusCode::CREATED, name).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// DELETE /projects/{name}
async fn delete_project(
    State(mut s): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    require_auth!(s, headers);
    s.db.delete_project(&name).await.ok();
    StatusCode::NO_CONTENT.into_response()
}

/// GET /projects/{name}/envs  -> YAML
async fn get_envs(
    State(mut s): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    require_auth!(s, headers);
    match s.db.get_envs(&name).await {
        Ok(envs) => {
            let yaml = serde_yaml::to_string(&envs).unwrap_or_default();
            (StatusCode::OK, [("content-type", "application/yaml")], yaml).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /projects/{name}/envs  body: { "envs": { "KEY": "val", ... } }
async fn set_envs(
    State(mut s): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<SetEnvsBody>,
) -> Response {
    require_auth!(s, headers);
    match s.db.set_envs(&name, &body.envs).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// DELETE /projects/{name}/envs/{key}
async fn delete_env(
    State(mut s): State<AppState>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response {
    require_auth!(s, headers);
    s.db.delete_env(&name, &key).await.ok();
    StatusCode::NO_CONTENT.into_response()
}

/// GET /health
async fn health() -> &'static str {
    "ok"
}

// -- Main

#[derive(Parser)]
#[command(name = "envd", about = "Environment variable daemon", version)]
struct Args {
    /// Path to config file
    config: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config_path = args.config.unwrap_or_else(|| "server.yml".into());
    let cfg = load_config(&config_path)?;

    let bind = cfg.config.bind.as_deref().unwrap_or("0.0.0.0:7878");

    let db = Db::connect(&cfg.config.backend, &cfg.storage).await?;

    let state = AppState {
        db,
        auth: cfg.config.auth,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{name}", delete(delete_project))
        .route("/projects/{name}/envs", get(get_envs).post(set_envs))
        .route("/projects/{name}/envs/{key}", delete(delete_env))
        .with_state(state);

    println!("✓ envd listening on {bind}");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

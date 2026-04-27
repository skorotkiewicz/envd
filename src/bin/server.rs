use std::{collections::HashMap, fs, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

// -- Config

#[derive(Debug, Deserialize)]
struct ServerConfig {
    config: Config,
    users: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Config {
    bind: Option<String>,
    db: String,
}

fn load_config(path: &str) -> anyhow::Result<ServerConfig> {
    let text = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&text)?)
}

// -- State

#[derive(Clone)]
struct AppState {
    db: PgPool,
    users: Arc<HashMap<String, String>>,
}

// -- Auth

fn auth(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|t| state.users.values().any(|v| v == t))
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

#[derive(Serialize, sqlx::FromRow)]
struct EnvRow {
    key: String,
    value: String,
}

#[derive(Deserialize)]
struct SetEnvsBody {
    envs: HashMap<String, String>,
}

// -- Handlers

/// GET /projects
async fn list_projects(State(s): State<AppState>, headers: HeaderMap) -> Response {
    require_auth!(s, headers);
    let rows: Vec<String> = sqlx::query_scalar("SELECT name FROM projects ORDER BY name")
        .fetch_all(&s.db)
        .await
        .unwrap_or_default();
    Json(rows).into_response()
}

/// POST /projects  body: { "name": "myapp" }
async fn create_project(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    require_auth!(s, headers);
    let name = match body["name"].as_str() {
        Some(n) => n.to_string(),
        None => return (StatusCode::BAD_REQUEST, "missing name").into_response(),
    };
    match sqlx::query("INSERT INTO projects (name) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(&name)
        .execute(&s.db)
        .await
    {
        Ok(_) => (StatusCode::CREATED, name).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// DELETE /projects/:name
async fn delete_project(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    require_auth!(s, headers);
    sqlx::query("DELETE FROM envs WHERE project = $1")
        .bind(&name)
        .execute(&s.db)
        .await
        .ok();
    sqlx::query("DELETE FROM projects WHERE name = $1")
        .bind(&name)
        .execute(&s.db)
        .await
        .ok();
    StatusCode::NO_CONTENT.into_response()
}

/// GET /projects/:name/envs  -> YAML
async fn get_envs(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    require_auth!(s, headers);
    let rows: Vec<EnvRow> =
        sqlx::query_as("SELECT key, value FROM envs WHERE project = $1 ORDER BY key")
            .bind(&name)
            .fetch_all(&s.db)
            .await
            .unwrap_or_default();

    let map: HashMap<String, String> = rows.into_iter().map(|r| (r.key, r.value)).collect();
    let yaml = serde_yaml::to_string(&map).unwrap_or_default();
    (StatusCode::OK, [("content-type", "application/yaml")], yaml).into_response()
}

/// POST /projects/:name/envs  body: { "envs": { "KEY": "val", ... } }
async fn set_envs(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(body): Json<SetEnvsBody>,
) -> Response {
    require_auth!(s, headers);

    // ensure project exists
    sqlx::query("INSERT INTO projects (name) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(&name)
        .execute(&s.db)
        .await
        .ok();

    for (key, value) in &body.envs {
        let res = sqlx::query(
            "INSERT INTO envs (project, key, value) VALUES ($1, $2, $3)
             ON CONFLICT (project, key) DO UPDATE SET value = $3, updated = now()",
        )
        .bind(&name)
        .bind(key)
        .bind(value)
        .execute(&s.db)
        .await;

        if let Err(e) = res {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    }

    StatusCode::NO_CONTENT.into_response()
}

/// DELETE /projects/:name/envs/:key
async fn delete_env(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((name, key)): Path<(String, String)>,
) -> Response {
    require_auth!(s, headers);
    sqlx::query("DELETE FROM envs WHERE project = $1 AND key = $2")
        .bind(&name)
        .bind(&key)
        .execute(&s.db)
        .await
        .ok();
    StatusCode::NO_CONTENT.into_response()
}

/// GET /health
async fn health() -> &'static str {
    "ok"
}

// -- Migrations

async fn migrate(db: &PgPool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS projects (
            id      SERIAL PRIMARY KEY,
            name    TEXT UNIQUE NOT NULL,
            created TIMESTAMPTZ DEFAULT now()
        )",
    )
    .execute(db)
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
    .execute(db)
    .await?;

    Ok(())
}

// -- Main

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_path = std::env::args().nth(1).unwrap_or("server.yml".into());
    let cfg = load_config(&config_path)?;

    let bind = cfg.config.bind.as_deref().unwrap_or("0.0.0.0:7878");

    let db = PgPool::connect(&cfg.config.db).await?;
    migrate(&db).await?;
    println!("✓ db connected");

    let state = AppState {
        db,
        users: Arc::new(cfg.users),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/:name", delete(delete_project))
        .route("/projects/:name/envs", get(get_envs).post(set_envs))
        .route("/projects/:name/envs/:key", delete(delete_env))
        .with_state(state);

    println!("✓ envd listening on {bind}");
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

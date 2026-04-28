use std::{collections::HashMap, env, fs, path::PathBuf, process::Command};

use anyhow::Context;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

// -- Config

#[derive(Debug, Deserialize, Serialize)]
struct ClientConfig {
    config: Config,
    projects: Option<HashMap<String, String>>, // name -> local path
}

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    endpoint: String,
    token: String,
}

fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/envd/client.yml")
}

fn load_config() -> anyhow::Result<ClientConfig> {
    let path = config_path();
    let text = fs::read_to_string(&path)
        .with_context(|| format!("config not found at {}", path.display()))?;
    Ok(serde_yaml::from_str(&text)?)
}

fn save_config(cfg: &ClientConfig) -> anyhow::Result<()> {
    let path = config_path();
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, serde_yaml::to_string(cfg)?)?;
    Ok(())
}

// -- Project resolution

fn resolve_project(projects: &HashMap<String, String>) -> anyhow::Result<String> {
    let cwd = env::current_dir()?;

    let mut best: Option<(usize, String)> = None;
    for (name, raw_path) in projects {
        let expanded = shellexpand::tilde(raw_path).to_string();
        let proj_path = PathBuf::from(&expanded);
        if cwd.starts_with(&proj_path) {
            let depth = proj_path.components().count();
            if best.as_ref().is_none_or(|(d, _)| depth > *d) {
                best = Some((depth, name.clone()));
            }
        }
    }

    best.map(|(_, name)| name)
        .context("no project matched current directory — use --project or run `env project add`")
}

fn get_project(explicit: Option<String>, cfg: &ClientConfig) -> anyhow::Result<String> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    let projects = cfg.projects.as_ref().cloned().unwrap_or_default();
    resolve_project(&projects)
}

// -- HTTP helpers

fn client(cfg: &Config) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert("authorization", cfg.token.parse().unwrap());
            h
        })
        .build()
        .unwrap()
}

// -- CLI

#[derive(Parser)]
#[command(name = "env", about = "Manage project envs via envd", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage projects
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    /// Set one or more envs: KEY=val KEY2=val2
    Set {
        #[arg(value_name = "KEY=val", required = true)]
        pairs: Vec<String>,

        #[arg(long, short)]
        project: Option<String>,
    },

    /// Get all envs (or a single key)
    Get {
        key: Option<String>,

        #[arg(long, short)]
        project: Option<String>,
    },

    /// Delete an env key
    Rm {
        key: String,

        #[arg(long, short)]
        project: Option<String>,
    },

    /// Run a command with envs injected
    Run {
        #[arg(required = true, last = true)]
        cmd: Vec<String>,

        #[arg(long, short)]
        project: Option<String>,
    },

    /// Print shell hook (add `eval "$(env hook zsh)"` to .zshrc)
    Hook { shell: String },
}

#[derive(Subcommand)]
enum ProjectAction {
    /// Register a project locally
    Add { name: String, path: String },
    /// List registered projects
    List,
    /// Remove a project locally
    Rm { name: String },
}

// -- Main

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        // -- project management
        Cmd::Project { action } => {
            let mut cfg = load_config()?;
            let projects = cfg.projects.get_or_insert_with(HashMap::new);

            match action {
                ProjectAction::Add { name, path } => {
                    let expanded = shellexpand::tilde(&path).to_string();
                    let abs = PathBuf::from(&expanded);
                    let abs = if abs.is_relative() {
                        env::current_dir()?.join(abs)
                    } else {
                        abs
                    };
                    let abs = abs.canonicalize().unwrap_or(abs);
                    let abs = abs.to_string_lossy().to_string();
                    projects.insert(name.clone(), abs.clone());
                    save_config(&cfg)?;
                    println!("✓ project '{name}' -> {abs}");
                }
                ProjectAction::List => {
                    if projects.is_empty() {
                        println!("no projects registered");
                    } else {
                        for (name, path) in projects {
                            println!("  {name:20} {path}");
                        }
                    }
                }
                ProjectAction::Rm { name } => {
                    projects.remove(&name);
                    save_config(&cfg)?;
                    println!("✓ removed '{name}'");
                }
            }
        }

        // -- set
        Cmd::Set { pairs, project } => {
            let cfg = load_config()?;
            let proj = get_project(project, &cfg)?;

            let mut envs: HashMap<String, String> = HashMap::new();
            for pair in pairs {
                let (k, v) = pair
                    .split_once('=')
                    .with_context(|| format!("invalid format '{pair}', expected KEY=val"))?;
                envs.insert(k.to_string(), v.to_string());
            }

            let url = format!("{}/projects/{}/envs", cfg.config.endpoint, proj);
            let res = client(&cfg.config)
                .post(&url)
                .json(&serde_json::json!({ "envs": envs }))
                .send()?;

            if res.status().is_success() {
                println!("✓ saved {} env(s) to '{proj}'", envs.len());
            } else {
                anyhow::bail!("server error: {}", res.status());
            }
        }

        // -- get
        Cmd::Get { key, project } => {
            let cfg = load_config()?;
            let proj = get_project(project, &cfg)?;

            let url = format!("{}/projects/{}/envs", cfg.config.endpoint, proj);
            let body = client(&cfg.config).get(&url).send()?.text()?;
            let envs: HashMap<String, String> = serde_yaml::from_str(&body)?;

            if let Some(k) = key {
                match envs.get(&k) {
                    Some(v) => println!("{v}"),
                    None => anyhow::bail!("key '{k}' not found in '{proj}'"),
                }
            } else {
                for (k, v) in &envs {
                    println!("{k}={v}");
                }
            }
        }

        // -- rm
        Cmd::Rm { key, project } => {
            let cfg = load_config()?;
            let proj = get_project(project, &cfg)?;

            let url = format!("{}/projects/{}/envs/{}", cfg.config.endpoint, proj, key);
            client(&cfg.config).delete(&url).send()?;
            println!("✓ deleted '{key}' from '{proj}'");
        }

        // -- run
        Cmd::Run { cmd, project } => {
            let cfg = load_config()?;
            let proj = get_project(project, &cfg)?;

            let url = format!("{}/projects/{}/envs", cfg.config.endpoint, proj);
            let body = client(&cfg.config).get(&url).send()?.text()?;
            let envs: HashMap<String, String> = serde_yaml::from_str(&body)?;

            let (bin, args) = cmd.split_first().context("empty command")?;
            let status = Command::new(bin).args(args).envs(&envs).status()?;
            std::process::exit(status.code().unwrap_or(1));
        }

        // -- hook
        Cmd::Hook { shell } => match shell.as_str() {
            "zsh" | "bash" => {
                println!(
                    r#"
# envd shell hook
_envd_hook() {{
  while IFS='=' read -r key val; do
    export "$key=$val"
  done < <(enve get 2>/dev/null)
}}
if [[ -n "$ZSH_VERSION" ]]; then
  autoload -U add-zsh-hook
  add-zsh-hook chpwd _envd_hook
else
  PROMPT_COMMAND="_envd_hook;$PROMPT_COMMAND"
fi
_envd_hook
"#
                );
            }
            _ => eprintln!("supported shells: zsh, bash"),
        },
    }

    Ok(())
}

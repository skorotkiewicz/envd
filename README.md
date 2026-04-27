# envd

A tiny server that stores per-project environment variables. A CLI client reads and writes them.

## Quick start

```bash
# start the server
docker compose up -d          # starts valkey (or use your own)
cargo run --bin envd server.yml

# configure the client
env project add myapp ~/Dev/myapp/

# daily use — project auto-detected from cwd
env set DATABASE_URL=postgres://localhost/myapp
env set API_KEY=secret
env get                       # prints all envs
env run cargo run             # runs with envs injected

# override project explicitly
env get --project myapp       # from anywhere, not just ~/Dev/myapp/
env set DEBUG=true --project backend
```

## Server config (`server.yml`)

```yaml
config:
  bind:     0.0.0.0:7878
  backend:  valkey          # valkey | postgres | sqlite
  auth:     secret_token    # shared API key

storage:
  valkey:   redis://localhost:6379
  # postgres: postgresql://user:pass@localhost/envd
  # sqlite:   "envd.db"
```

Pick one backend:
- **valkey** — fast, in-memory, optional persistence
- **postgres** — durable, team-friendly
- **sqlite** — zero setup, file-based

## Client config (`~/.config/envd/client.yml`)

```yaml
config:
  endpoint: http://localhost:7878
  token:    secret_token

projects:
  myapp:   ~/Dev/myapp/
  backend: ~/Dev/backend/
```

Project is auto-detected from `cwd` (deepest prefix match).

## Commands

| Command | Description |
|---|---|
| `env project add NAME PATH` | register project |
| `env set KEY=val [KEY2=val2]` | set env(s) |
| `env get [KEY]` | get all or one env |
| `env rm KEY` | delete one env |
| `env run <cmd>` | run command with envs injected |

Override project: `--project myapp`

## License

MIT

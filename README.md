# envd

Store per-project environment variables in one place. Share them across machines. Auto-load them when you `cd` into a project.

## Install

```bash
# Arch Linux
yay -S envd

# From source
cargo install --path .
```

## First time

**1. Start the server**

```bash
docker compose up -d   # starts valkey (or use your own redis/valkey)
```

Create `server.yml`:

```yaml
config:
  bind:     0.0.0.0:7878
  backend:  valkey
  auth:     change-me

storage:
  valkey: redis://localhost:6379
  # postgres: postgresql://user:pass@localhost/envd
  # sqlite:   "envd.db"
```

```bash
envd server.yml
```

**2. Configure the client**

Create `~/.config/envd/client.yml`:

```yaml
config:
  endpoint: http://localhost:7878
  token:    change-me
```

**3. Register your project**

```bash
cd ~/Dev/myapp
enve project add myapp .
```

**4. Save some envs**

```bash
enve set DATABASE_URL=postgres://localhost/myapp
enve set API_KEY=secret
```

## Daily use

```bash
cd ~/Dev/myapp          # project auto-detected
enve get                # see all envs
enve set DEBUG=true     # add/update one
enve run -- cargo run   # run anything with envs injected
```

From another directory:

```bash
enve get API_KEY=secret --project myapp
```

## Shell hook (optional)

Add to `~/.zshrc` or `~/.bashrc`:

```bash
eval "$(enve hook zsh)"
```

Now `cd ~/Dev/myapp` automatically exports envs — `cargo run`, `bun test.js`, etc. just work.

## Backends

Pick one in `server.yml`:

| Backend | Config | Use case |
|---|---|---|
| **Valkey** | `valkey: redis://localhost:6379` | Fast, shared cache |
| **PostgreSQL** | `postgres: postgresql://user:pass@localhost/envd` | Durable, team-friendly |
| **SQLite** | `sqlite: "envd.db"` | Zero setup, file-based |

## Shell completions (optional)

Add to `~/.zshrc` or `~/.bashrc`:

```bash
eval "$(enve complete zsh)"   # or: zsh, fish | source
```

## Commands

| Command | Description |
|---|---|
| `enve project add NAME PATH` | Register a project |
| `enve project list` | Show registered projects |
| `enve project rm NAME` | Remove a project |
| `enve set KEY=val` | Save one or more envs |
| `enve get [KEY]` | Show all envs (or one) |
| `enve rm KEY` | Delete an env |
| `enve run -- <cmd>` | Run command with envs injected |

## License

MIT

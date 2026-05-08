# envd

Store per-project environment variables in one place. Share them across machines. Auto-load them when you `cd` into a project. Isolate environments using **vaults**.

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
enve get API_KEY --project myapp
```

## Vaults

Each project supports multiple **vaults** — independent sets of environment variables. Think of them as profiles or environments (e.g., `0` for local dev, `staging`, `production`).

The default vault is `0`. If you don't specify a vault, the active vault for the project is used.

### Switch active vault

```bash
# Switch to the 'production' vault (saved in client config)
enve vault switch production

# All standard commands now use the 'production' vault
enve set DATABASE_URL=postgres://prod-host/myapp
enve get                              # shows production envs
```

### List and inspect vaults

```bash
# List all vaults that exist for a project (* = active)
enve vault list

# Peek at another vault's envs without switching
enve vault get 0
enve vault get staging
```

### Delete a vault

```bash
# Removes the vault and all its envs from the server
enve vault rm staging
```

### One-off vault usage

Use the `--vault` flag on any standard command to target a specific vault without switching your active vault:

```bash
enve get --vault 0 DATABASE_URL
enve set --vault staging DEBUG=true
enve rm --vault production OLD_KEY
enve run --vault staging -- cargo test
```

### Vault configuration

The active vault is persisted per-project in `~/.config/envd/client.yml`:

```yaml
config:
  endpoint: http://localhost:7878
  token:    change-me

projects:
  myapp:   /home/mod/Dev/myapp
  backend: /home/mod/Dev/backend

# Active vault per project (default is "0" if omitted)
vaults:
  myapp:   "production"
  # backend: "0"
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
| `enve project list` | Show registered projects & active vault |
| `enve project rm NAME` | Remove a project |
| `enve set KEY=val [--vault ID]` | Save one or more envs |
| `enve get [KEY] [--vault ID]` | Show all envs (or one) |
| `enve rm KEY [--vault ID]` | Delete an env |
| `enve run -- <cmd> [--vault ID]` | Run command with envs injected |
| `enve vault switch ID` | Switch active vault for project |
| `enve vault list` | List vaults for project (* = active) |
| `enve vault get ID` | Show envs from a specific vault |
| `enve vault rm ID` | Remove a vault and all its envs |

## License

MIT

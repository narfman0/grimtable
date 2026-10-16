# Deploying

One box today: **`grimtable-dev`** on Fly.io, <https://grimtable-dev.fly.dev>.

## Shape

- **One image** (`Dockerfile`): the `server` binary, the built client in
  `client/dist` (the server serves it), and the data files the server reads
  at runtime: `data/40kdc-data/data` and the BSData `*.json` catalogues.
  It builds from the local checkout, so initialise the submodules first.
- **Postgres on Neon**, in the same Neon project as `fullpod-dev` but a
  database of its own, `grimtable_dev`. The server creates its tables on
  boot (`crates/server/src/db.rs`). `DATABASE_URL` is a Fly secret.
- **Scales to zero.** `min_machines_running = 0` with auto stop/start: an
  idle box stops, and the first request starts it again in a couple of
  seconds. Live rooms are in memory, but every game's state and action log
  is in Postgres and reloads on the next connection, so a stop loses
  nothing. Fly does not stop a machine with open connections, so a game
  in progress keeps it up.
- **One machine.** Rooms fan out in memory, so two machines would split a
  game's players. More than one needs routing by game id first
  (`docs/design.md`).

## Environment

- `DATABASE_URL`: required. `postgres://…?sslmode=require`.
- `GRIMTABLE_ADDR`: `0.0.0.0:8080` in the image.
- `GRIMTABLE_ROOT`: `/app` in the image.
- `GRIMTABLE_SECURE_COOKIES`: `1` on Fly (HTTPS), so the session cookie is
  `Secure`. Leave unset for plain-HTTP local runs.
- `RUST_LOG`: tracing filter.

## First time

```sh
fly apps create grimtable-dev
neon databases create --project-id <fullpod's project> --name grimtable_dev
echo "DATABASE_URL=$(neon connection-string --project-id <…> --database-name grimtable_dev)" \
  | fly secrets import -a grimtable-dev   # the URL is never printed
fly deploy
```

## Every deploy

```sh
git submodule update --init
fly deploy
curl -sf https://grimtable-dev.fly.dev/health
```

## Local dev box (podman)

The same image on one host, with Postgres in a container beside it, as
systemd user services (quadlets in `~/.config/containers/systemd`:
`grimtable.network`, `grimtable-pg.container`, `grimtable-dev.container`;
secrets in `~/.config/grimtable/dev.env`, mode 600). The app listens on
port 8095 over plain HTTP, so `GRIMTABLE_SECURE_COOKIES` stays unset.

```sh
podman build -t grimtable-dev:local .
systemctl --user restart grimtable-dev
curl -sf localhost:8095/health
```

The same Postgres runs the server tests:
`set -a; . ~/.config/grimtable/dev.env; set +a; cargo test -p server`.

# Panda Server

High-performance Rust notes server for the **Panda** product. Markdown is the
canonical note body; hot paths use compact save ACKs, content-addressed storage,
inventory sync, and WebSocket sync hints.

## Quick start

```sh
cd panda-server
# If the repo disk is low on space:
#   set CARGO_TARGET_DIR to a larger volume
cargo run -p server -- --config config/default.yml
```

Default bind: `127.0.0.1:8787`  
Bootstrap login: `admin` / `admin123` (change after first login)

Data and blobs default under `./data`. Schema SQL lives in [`migrations/`](migrations/)
and is applied automatically on startup.

## Docker

```sh
docker build -t panda-server .
docker run --rm -p 8787:8787 -v panda-data:/data panda-server
```

The image listens on `0.0.0.0:8787` and stores SQLite + blobs under `/data`.
Override bind or database with `PANDA_BIND` / `PANDA_DATABASE_URL`, or mount a
custom config and pass `--config`.

Health check from the host (the image is distroless and has no shell/`curl`):

```sh
curl -fsS http://127.0.0.1:8787/api/v1/health
```

## Protocol (`/api/v1`)

The machine-readable contract is [`openapi.yaml`](openapi.yaml). Desktop and
other long-lived clients should store an `apiUrl` (for example
`https://notes.example/api/v1`) plus a generated API token, then send
`Authorization: Bearer <token>` on every request. Password login is only used
to bootstrap or manage tokens; it is not the recommended client connection
method.

| Method | Path | Notes |
|--------|------|-------|
| GET | `/health` | Liveness |
| POST | `/auth/login` | Returns Bearer `session_token` |
| GET | `/notebooks` | Notebook tree |
| GET | `/memos` | Summaries only (keyset cursor) |
| POST | `/memos` | Create from markdown |
| POST | `/memos/{id}/open` | Detail + optional edit lease |
| POST | `/memos/{id}/save` | `markdown_full` / `markdown_patch` to ACK |
| GET | `/contents/{hash}` | CAS markdown bytes |
| GET | `/sync/inventory` | Notebooks + memo etag inventory |
| GET | `/sync/pull` | Changelog |
| POST | `/sync/push` | Offline batch |
| GET | `/sync/ws` | First frame = auth; then `SyncHint` |
| GET | `/exports/markdown.zip` | Markdown ZIP export |
| GET | `/metrics` | Prometheus text metrics |
| POST | `/mcp` or `/api/v1/mcp` | MCP Streamable HTTP for Hermes and other agents (Bearer) |

Wire format: JSON by default; set `Content-Type` / `Accept: application/x-protobuf` for prost.

More detail:

- [`docs/README.md`](docs/README.md) — doc index
- [`docs/client-adapter.md`](docs/client-adapter.md) — desktop adapter notes
- [`docs/mcp.md`](docs/mcp.md) — agent / MCP setup
- [`docs/multitenancy.md`](docs/multitenancy.md) — workspace isolation

## Workspace crates

`proto`, `domain`, `store`, `blob`, `auth`, `sync`, `mcp`, `server` (bin `panda`)

The domain crate is named `domain` (not `core`) to avoid clashing with Rust's
`core` crate inside proc-macros such as `tokio::main` / `async_trait`.

## Config

See [`config/default.yml`](config/default.yml). Override with `PANDA_BIND`,
`PANDA_DATABASE_URL`, `PANDA_CONFIG`. Container image defaults are in
[`config/docker.yml`](config/docker.yml).

## Tests

```sh
cargo test -p domain -p server
```

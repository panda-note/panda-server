# Panda Server

High-performance Rust notes server for the **Panda** product. Markdown is the
canonical note body; hot paths use compact save ACKs, content-addressed storage,
inventory sync, and WebSocket sync hints.

## Install

Published builds ship on every `v*` tag:

- GitHub Release tarballs: [Releases](https://github.com/panda-note/panda-server/releases)
- Container image: `ghcr.io/panda-note/panda-server:<tag>` and `:latest`

Tarball binaries target Debian bookworm glibc (≥ 2.36) and only need
`libc` / `libm` / `libgcc_s` at runtime (SQLite is bundled).

Bootstrap login: `admin` / `admin123` (change after first login).

### Container (GHCR)

```sh
docker pull ghcr.io/panda-note/panda-server:v0.1.0

docker run --rm -p 8787:8787 \
  -v panda-data:/data \
  ghcr.io/panda-note/panda-server:v0.1.0
```

The image listens on `0.0.0.0:8787` and stores SQLite + blobs under `/data`.
Override with `PANDA_BIND` / `PANDA_DATABASE_URL`, or mount a custom config and
pass `--config`. Image defaults live in [`config/docker.yml`](config/docker.yml).

Health check from the host (the image is distroless and has no shell/`curl`):

```sh
curl -fsS http://127.0.0.1:8787/api/v1/health
```

### Binary (GitHub Release)

```sh
# amd64
curl -fsSL -o panda-linux-amd64.tar.gz \
  https://github.com/panda-note/panda-server/releases/download/v0.1.0/panda-linux-amd64.tar.gz
tar -xzf panda-linux-amd64.tar.gz
chmod +x panda

# arm64: use panda-linux-arm64.tar.gz instead

./panda --config config/default.yml
```

Default bind for the sample config: `127.0.0.1:8787`. Data and blobs default
under `./data`. Schema SQL in [`migrations/`](migrations/) is applied on startup.

## Protocol (`/api/v1`)

**Protocol attribution.** The Panda `/api/v1` protocol design heavily references
[EdgeEver](https://github.com/tianma-if/edgeever)—especially the notebook/note
resource model, Markdown-facing API, token auth, incremental sync, export, and
MCP concepts. Panda Server is a separate Rust implementation; this is not a
fork of that codebase and wire compatibility is not implied.

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
| POST | `/auth/register` | Public signup when `auth.allow_registration` (isolated workspace) |
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
- Protocol design reference: https://github.com/tianma-if/edgeever

## Develop from source

```sh
cd panda-server
# If the repo disk is low on space:
#   set CARGO_TARGET_DIR to a larger volume
cargo run -p server -- --config config/default.yml
cargo test -p domain -p server
```

Local image build (optional; prefer GHCR for deploy):

```sh
docker build -t panda-server .
docker run --rm -p 8787:8787 -v panda-data:/data panda-server
```

## Workspace crates

`proto`, `domain`, `store`, `blob`, `auth`, `sync`, `mcp`, `server` (bin `panda`)

The domain crate is named `domain` (not `core`) to avoid clashing with Rust's
`core` crate inside proc-macros such as `tokio::main` / `async_trait`.

## Config

See [`config/default.yml`](config/default.yml). Override with `PANDA_BIND`,
`PANDA_DATABASE_URL`, `PANDA_CONFIG`.

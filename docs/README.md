# Documentation

English docs for running and integrating **Panda Server**.

Protocol shapes and client/sync concepts heavily reference
[EdgeEver](https://github.com/tianma-if/edgeever). See the root
[README Protocol section](../README.md#protocol-apiv1) for the canonical
attribution.

| Doc | Description |
|-----|-------------|
| [client-adapter.md](./client-adapter.md) | Desktop / client mapping to `/api/v1` |
| [mcp.md](./mcp.md) | MCP Streamable HTTP for agents |
| [multitenancy.md](./multitenancy.md) | Workspace isolation and auth boundaries |

## Schema

SQL migrations live at the repository root under [`../migrations/`](../migrations/).
The store crate embeds and applies them automatically on startup. Keep new
schema changes as numbered `.sql` files there; do not nest them under
`crates/`.

## Config and Docker

- Local defaults: [`../config/default.yml`](../config/default.yml)
  (`127.0.0.1:8787`, `./data`)
- Container defaults: [`../config/docker.yml`](../config/docker.yml)
  (`0.0.0.0:8787`, `/data`)

See the root [README](../README.md) for quick start and Docker usage.

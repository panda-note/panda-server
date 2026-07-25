# Panda MCP for agents

Panda exposes a stateless MCP Streamable HTTP endpoint for Hermes and other
MCP clients. Both URLs are equivalent:

- `POST /mcp`
- `POST /api/v1/mcp`

Every request must carry `Authorization: Bearer <token>`. Prefer a dedicated
API token over a password session. The recommended scopes are:

```json
["memos:read", "memos:write", "notebooks:read", "notebooks:write"]
```

The server supports MCP protocol versions `2025-06-18`, `2025-03-26`, and
`2024-11-05`. It is stateless: clients do not need to retain an MCP session ID,
and a successful notification receives HTTP 204.

## Hermes configuration

Add the server to `~/.hermes/mcp_servers.yaml` (or the equivalent
`mcp_servers` section in Hermes configuration):

```yaml
mcp_servers:
  panda:
    url: "https://notes.example/mcp"
    headers:
      Authorization: "Bearer pnda_REPLACE_WITH_API_TOKEN"
```

For a local server, use `http://127.0.0.1:8787/mcp`. The token is created once
through `POST /api/v1/api-tokens`; its plaintext value is returned only in that
creation response.

## Tools

| Tool | Purpose | Scope |
|---|---|---|
| `list_notebooks` | List notebook hierarchy and IDs | `notebooks:read` |
| `create_notebook` | Create a notebook or child notebook | `notebooks:write` |
| `list_memos` | List memo summaries with cursor paging | `memos:read` |
| `search_memos` | Search before adding possibly duplicate content | `memos:read` |
| `get_memo` | Read full Markdown, revision, and ETag | `memos:read` |
| `create_memo` | Create Markdown content, defaulting to Inbox | `memos:write` |
| `update_memo` | Replace Markdown under ETag concurrency control | `memos:write` |
| `append_to_memo` | Append a focused section or block | `memos:write` |
| `replace_in_memo` | Exact replacement; ambiguous matches fail safely | `memos:write` |
| `format_memo` | Conservative deterministic Markdown cleanup | `memos:write` |
| `list_todos` / `get_todo` | Read independent Todos | `memos:read` |
| `create_todo` / `update_todo` | Create or edit a Todo | `memos:write` |
| `set_todo_completed` | Complete or restore a Todo | `memos:write` |

Destructive delete tools are intentionally not exposed in the first version.
Agents can add, organize, edit, and complete content without being able to
permanently remove user data.

`format_memo` does not ask a model to rewrite prose. It only normalizes line
endings, trailing whitespace outside fenced code, ATX heading spacing,
repeated blank lines, and the final newline. Fenced code content is preserved.

## Safe editing workflow

1. Use `search_memos` before `create_memo`.
2. Use `get_memo` before broad edits and retain the returned ETag.
3. Prefer `append_to_memo` or `replace_in_memo` for a focused change.
4. Pass a stable `idempotency_key` to Memo write tools and reuse it when
   retrying the same operation.
5. On a conflict tool result (`isError: true`, such as `revision_conflict` or
   `content_conflict`), read the memo again and reconcile; never silently
   overwrite the remote revision.

Successful writes enter Panda's normal `sync_changes` stream and publish the
same synchronization hint used by REST writes, so connected desktop and mobile
clients refresh promptly.

## Protocol smoke test

```sh
curl -X POST http://127.0.0.1:8787/mcp \
  -H "Authorization: Bearer pnda_REPLACE_WITH_API_TOKEN" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke-test","version":"1"}}}'
```

Server metadata is available separately at `GET /api/v1/mcp/info` and is not
part of the MCP transport.

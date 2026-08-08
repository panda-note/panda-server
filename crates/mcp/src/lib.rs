//! Stateless MCP Streamable HTTP domain adapter for Panda.

use auth::{AuthContext, AuthService};
use blob::BlobStore;
use domain::{new_id, normalize_markdown, PandaError, PandaResult};
use serde_json::{json, Value};
use std::sync::Arc;
use store::{MemoListQuery, Store, TodoCreate, TodoListQuery, TodoUpdate};
use sync::{hint_after_change, NotifyBus};

pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &[LATEST_PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

pub struct McpHandler {
    pub store: Store,
    pub blobs: BlobStore,
    pub bus: Arc<dyn NotifyBus>,
}

impl McpHandler {
    pub async fn handle(&self, ctx: &AuthContext, body: Value) -> Value {
        if let Value::Array(messages) = body {
            if messages.is_empty() {
                return rpc_error(Value::Null, -32600, "empty JSON-RPC batch");
            }
            let mut responses = Vec::new();
            for message in messages {
                let response = self.handle_message(ctx, message).await;
                if !response.is_null() {
                    responses.push(response);
                }
            }
            return if responses.is_empty() {
                Value::Null
            } else {
                Value::Array(responses)
            };
        }
        self.handle_message(ctx, body).await
    }

    async fn handle_message(&self, ctx: &AuthContext, body: Value) -> Value {
        let Some(object) = body.as_object() else {
            return rpc_error(Value::Null, -32600, "invalid JSON-RPC request");
        };
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return rpc_error(Value::Null, -32600, "jsonrpc must be 2.0");
        }
        let id = object.get("id").cloned();
        let response_id = id.clone().unwrap_or(Value::Null);
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            return rpc_error(response_id, -32600, "method is required");
        };

        let response = match method {
            "initialize" => {
                let requested = body
                    .pointer("/params/protocolVersion")
                    .and_then(Value::as_str);
                let protocol_version = requested
                    .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(version))
                    .unwrap_or(LATEST_PROTOCOL_VERSION);
                rpc_result(
                    response_id,
                    json!({
                        "protocolVersion": protocol_version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": {
                            "name": "panda",
                            "title": "Panda Notes",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "instructions": "Use search_memos before creating content. Read a memo before replacing it, pass its etag when writing, and prefer append_to_memo or replace_in_memo for focused edits."
                    }),
                )
            }
            "ping" => rpc_result(response_id, json!({})),
            "tools/list" => rpc_result(response_id, json!({ "tools": tool_definitions() })),
            "tools/call" => {
                let Some(name) = body.pointer("/params/name").and_then(Value::as_str) else {
                    return if id.is_none() {
                        Value::Null
                    } else {
                        rpc_error(response_id, -32602, "params.name is required")
                    };
                };
                let args = body
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !args.is_object() {
                    return if id.is_none() {
                        Value::Null
                    } else {
                        rpc_error(response_id, -32602, "params.arguments must be an object")
                    };
                }
                let result = match self.dispatch_tool(ctx, name, args).await {
                    Ok(value) => {
                        if let Some(kind) = mutation_kind(name) {
                            hint_after_change(
                                self.bus.as_ref(),
                                &self.store,
                                &ctx.workspace_id,
                                vec![kind.to_string()],
                            )
                            .await;
                        }
                        tool_success(value)
                    }
                    Err(error) => tool_failure(error),
                };
                rpc_result(response_id, result)
            }
            "notifications/initialized" | "notifications/cancelled" => Value::Null,
            _ if id.is_none() => Value::Null,
            _ => rpc_error(response_id, -32601, &format!("method not found: {method}")),
        };
        if id.is_none() {
            Value::Null
        } else {
            response
        }
    }

    async fn dispatch_tool(
        &self,
        ctx: &AuthContext,
        name: &str,
        args: Value,
    ) -> PandaResult<Value> {
        let required_scope = match name {
            "list_notebooks" => "notebooks:read",
            "create_notebook" => "notebooks:write",
            "list_memos" | "search_memos" | "get_memo" | "list_todos" | "get_todo" => "memos:read",
            _ => "memos:write",
        };
        AuthService::require_scope(ctx, required_scope)?;
        let workspace_id = &ctx.workspace_id;

        match name {
            "list_notebooks" => Ok(json!({
                "items": self.store.notebooks().list(workspace_id).await?
            })),
            "create_notebook" => {
                let notebook = self
                    .store
                    .notebooks()
                    .create(
                        workspace_id,
                        required_string(&args, "name")?,
                        optional_string(&args, "parent_id").as_deref(),
                        bounded_i32(&args, "sort_order", 0)?,
                    )
                    .await?;
                json_value(notebook)
            }
            "list_memos" | "search_memos" => {
                let query = MemoListQuery {
                    notebook_id: optional_string(&args, "notebook_id"),
                    trash: args.get("trash").and_then(Value::as_bool).unwrap_or(false),
                    q: if name == "search_memos" {
                        Some(required_string(&args, "query")?.to_string())
                    } else {
                        optional_string(&args, "query")
                    },
                    limit: bounded_limit(&args, 50),
                    cursor: optional_string(&args, "cursor"),
                };
                let (items, next_cursor) = self.store.memos().list(workspace_id, query).await?;
                Ok(json!({ "items": items, "next_cursor": next_cursor }))
            }
            "get_memo" => {
                self.memo_document(workspace_id, required_string(&args, "id")?)
                    .await
            }
            "create_memo" => {
                let markdown = required_string(&args, "markdown")?;
                let notebook_id = match optional_string(&args, "notebook_id") {
                    Some(id) => id,
                    None => {
                        self.store
                            .notebooks()
                            .default_inbox_id(workspace_id)
                            .await?
                    }
                };
                let tags = string_array(&args, "tags")?;
                let blobs = self.blobs.clone();
                let detail = self
                    .store
                    .memos()
                    .create(
                        workspace_id,
                        &ctx.user_id,
                        &notebook_id,
                        optional_string(&args, "title"),
                        markdown,
                        &tags,
                        move |bytes| {
                            let blobs = blobs.clone();
                            let data = bytes.to_vec();
                            Box::pin(async move { blobs.put(&data).await })
                        },
                    )
                    .await?;
                json_value(detail)
            }
            "update_memo" => {
                let id = required_string(&args, "id")?;
                let markdown = required_string(&args, "markdown")?;
                let etag = required_string(&args, "etag")?;
                self.save_memo(
                    ctx,
                    id,
                    markdown,
                    etag,
                    optional_string(&args, "title"),
                    optional_string_array(&args, "tags")?,
                    optional_string(&args, "idempotency_key"),
                )
                .await
            }
            "append_to_memo" => {
                let id = required_string(&args, "id")?;
                let summary = self.memo_summary(workspace_id, id).await?;
                let current = self.read_memo(workspace_id, id).await?;
                let content = required_string(&args, "content")?.trim();
                let heading = optional_string(&args, "heading");
                let addition = match heading {
                    Some(heading) => format!("## {}\n\n{}", heading.trim(), content),
                    None => content.to_string(),
                };
                let separator = args
                    .get("separator")
                    .and_then(Value::as_str)
                    .unwrap_or("\n\n");
                let markdown = if current.trim().is_empty() {
                    format!("{}\n", addition)
                } else {
                    format!("{}{}{}\n", current.trim_end(), separator, addition)
                };
                let etag = optional_string(&args, "etag").unwrap_or(summary.etag);
                self.save_memo(
                    ctx,
                    id,
                    &markdown,
                    &etag,
                    None,
                    None,
                    optional_string(&args, "idempotency_key"),
                )
                .await
            }
            "replace_in_memo" => {
                let id = required_string(&args, "id")?;
                let summary = self.memo_summary(workspace_id, id).await?;
                let find = required_string(&args, "find")?;
                if find.is_empty() {
                    return Err(PandaError::invalid("find must not be empty"));
                }
                let current = self.read_memo(workspace_id, id).await?;
                let occurrences = current.matches(find).count();
                if occurrences == 0 {
                    return Err(PandaError::not_found("text to replace was not found"));
                }
                let replace_all = args
                    .get("replace_all")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if occurrences > 1 && !replace_all {
                    return Err(PandaError::invalid(format!(
                        "find matched {occurrences} times; set replace_all=true or use a more specific value"
                    )));
                }
                let replacement = required_string(&args, "replacement")?;
                let markdown = if replace_all {
                    current.replace(find, replacement)
                } else {
                    current.replacen(find, replacement, 1)
                };
                let etag = optional_string(&args, "etag").unwrap_or(summary.etag);
                self.save_memo(
                    ctx,
                    id,
                    &markdown,
                    &etag,
                    None,
                    None,
                    optional_string(&args, "idempotency_key"),
                )
                .await
            }
            "format_memo" => {
                let id = required_string(&args, "id")?;
                let summary = self.memo_summary(workspace_id, id).await?;
                let current = self.read_memo(workspace_id, id).await?;
                let markdown = format_markdown(&current);
                if markdown == current {
                    return Ok(
                        json!({ "changed": false, "memo": self.memo_document(workspace_id, id).await? }),
                    );
                }
                let etag = optional_string(&args, "etag").unwrap_or(summary.etag);
                let saved = self
                    .save_memo(
                        ctx,
                        id,
                        &markdown,
                        &etag,
                        None,
                        None,
                        optional_string(&args, "idempotency_key"),
                    )
                    .await?;
                Ok(json!({ "changed": true, "save": saved, "markdown": markdown }))
            }
            "list_todos" => {
                let query = TodoListQuery {
                    filter: optional_string(&args, "filter").or_else(|| Some("all".into())),
                    limit: bounded_limit(&args, 50),
                    cursor: optional_string(&args, "cursor"),
                };
                let (items, next_cursor) = self.store.todos().list(workspace_id, query).await?;
                Ok(json!({ "items": items, "next_cursor": next_cursor }))
            }
            "get_todo" => json_value(
                self.store
                    .todos()
                    .get(workspace_id, required_string(&args, "id")?)
                    .await?,
            ),
            "create_todo" => json_value(
                self.store
                    .todos()
                    .create(
                        workspace_id,
                        TodoCreate {
                            id: None,
                            title: required_string(&args, "title")?.to_string(),
                            note: optional_string(&args, "note").unwrap_or_default(),
                            status: optional_string(&args, "status"),
                            due_date: optional_string(&args, "due_date"),
                            priority: args.get("priority").and_then(Value::as_i64),
                            linked_memo_id: optional_string(&args, "linked_memo_id"),
                        },
                    )
                    .await?,
            ),
            "update_todo" => {
                let id = required_string(&args, "id")?;
                let current = self.store.todos().get(workspace_id, id).await?;
                let updated = self
                    .store
                    .todos()
                    .update(
                        workspace_id,
                        id,
                        TodoUpdate {
                            title: optional_string(&args, "title"),
                            note: optional_string(&args, "note"),
                            status: optional_string(&args, "status"),
                            due_date: nullable_string(&args, "due_date")?,
                            priority: args.get("priority").and_then(Value::as_i64),
                            linked_memo_id: nullable_string(&args, "linked_memo_id")?,
                            base_revision: Some(
                                args.get("revision")
                                    .and_then(Value::as_i64)
                                    .unwrap_or(current.revision),
                            ),
                            if_match_etag: Some(
                                optional_string(&args, "etag").unwrap_or(current.etag),
                            ),
                        },
                    )
                    .await?;
                json_value(updated)
            }
            "set_todo_completed" => {
                let id = required_string(&args, "id")?;
                let completed = args
                    .get("completed")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let current = self.store.todos().get(workspace_id, id).await?;
                let updated = self
                    .store
                    .todos()
                    .update(
                        workspace_id,
                        id,
                        TodoUpdate {
                            title: None,
                            note: None,
                            status: Some(if completed { "completed" } else { "open" }.into()),
                            due_date: None,
                            priority: None,
                            linked_memo_id: None,
                            base_revision: Some(current.revision),
                            if_match_etag: Some(current.etag),
                        },
                    )
                    .await?;
                json_value(updated)
            }
            _ => Err(PandaError::invalid(format!("unknown tool: {name}"))),
        }
    }

    async fn memo_summary(&self, workspace_id: &str, id: &str) -> PandaResult<proto::MemoSummary> {
        self.store
            .memos()
            .get_summary(workspace_id, id)
            .await?
            .ok_or_else(|| PandaError::not_found("memo not found"))
    }

    async fn read_memo(&self, workspace_id: &str, id: &str) -> PandaResult<String> {
        let blobs = self.blobs.clone();
        self.store
            .memos()
            .read_markdown(workspace_id, id, move |hash| {
                let blobs = blobs.clone();
                Box::pin(async move { blobs.get(&hash).await })
            })
            .await
    }

    async fn memo_document(&self, workspace_id: &str, id: &str) -> PandaResult<Value> {
        let summary = self.memo_summary(workspace_id, id).await?;
        let markdown = self.read_memo(workspace_id, id).await?;
        Ok(json!({ "summary": summary, "markdown": markdown }))
    }

    async fn save_memo(
        &self,
        ctx: &AuthContext,
        id: &str,
        markdown: &str,
        etag: &str,
        title: Option<String>,
        tags: Option<Vec<String>>,
        idempotency_key: Option<String>,
    ) -> PandaResult<Value> {
        let blobs = self.blobs.clone();
        let blobs_for_read = self.blobs.clone();
        let ack = self
            .store
            .memos()
            .save(
                &ctx.workspace_id,
                &ctx.user_id,
                id,
                Some(etag),
                None,
                None,
                None,
                &idempotency_key.unwrap_or_else(new_id),
                Some(proto::SaveBody::MarkdownFull(markdown.to_string())),
                title,
                tags,
                None,
                None,
                None,
                move |bytes| {
                    let blobs = blobs.clone();
                    let data = bytes.to_vec();
                    Box::pin(async move { blobs.put(&data).await })
                },
                move |hash| {
                    let blobs = blobs_for_read.clone();
                    Box::pin(async move { blobs.get(&hash).await })
                },
            )
            .await?;
        Ok(json!({ "save": ack, "memo": self.memo_document(&ctx.workspace_id, id).await? }))
    }
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_success(value: Value) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": false
    })
}

fn mutation_kind(tool_name: &str) -> Option<&'static str> {
    match tool_name {
        "create_notebook" => Some("notebook"),
        "create_memo" | "update_memo" | "append_to_memo" | "replace_in_memo" | "format_memo" => {
            Some("memo")
        }
        "create_todo" | "update_todo" | "set_todo_completed" => Some("todo"),
        _ => None,
    }
}

fn tool_failure(error: PandaError) -> Value {
    let details = error
        .details_json
        .as_deref()
        .and_then(|details| serde_json::from_str::<Value>(details).ok());
    json!({
        "content": [{ "type": "text", "text": error.to_string() }],
        "structuredContent": {
            "error": {
                "code": error.code.as_str(),
                "message": error.message,
                "retryable": error.code.retryable(),
                "current_etag": error.current_etag,
                "details": details
            }
        },
        "isError": true
    })
}

fn json_value(value: impl serde::Serialize) -> PandaResult<Value> {
    serde_json::to_value(value).map_err(|error| PandaError::internal(error.to_string()))
}

fn required_string<'a>(args: &'a Value, key: &str) -> PandaResult<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| PandaError::invalid(format!("{key} is required")))
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn nullable_string(args: &Value, key: &str) -> PandaResult<Option<Option<String>>> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(value)) => Ok(Some(Some(value.clone()))),
        Some(_) => Err(PandaError::invalid(format!(
            "{key} must be a string or null"
        ))),
    }
}

fn string_array(args: &Value, key: &str) -> PandaResult<Vec<String>> {
    optional_string_array(args, key).map(Option::unwrap_or_default)
}

fn optional_string_array(args: &Value, key: &str) -> PandaResult<Option<Vec<String>>> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let array = value
        .as_array()
        .ok_or_else(|| PandaError::invalid(format!("{key} must be an array of strings")))?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| PandaError::invalid(format!("{key} must contain only strings")))
        })
        .collect::<PandaResult<Vec<_>>>()
        .map(Some)
}

fn bounded_limit(args: &Value, default: i64) -> i64 {
    args.get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(default)
        .clamp(1, 200)
}

fn bounded_i32(args: &Value, key: &str, default: i32) -> PandaResult<i32> {
    let Some(value) = args.get(key) else {
        return Ok(default);
    };
    let value = value
        .as_i64()
        .ok_or_else(|| PandaError::invalid(format!("{key} must be an integer")))?;
    i32::try_from(value).map_err(|_| PandaError::invalid(format!("{key} is out of range")))
}

fn format_markdown(markdown: &str) -> String {
    let normalized = normalize_markdown(markdown);
    let mut output = Vec::new();
    let mut blank_lines = 0;
    let mut in_fence = false;
    for raw_line in normalized.lines() {
        if in_fence {
            output.push(raw_line.to_string());
            if raw_line.trim_start().starts_with("```") || raw_line.trim_start().starts_with("~~~")
            {
                in_fence = false;
            }
            continue;
        }

        let mut line = raw_line.trim_end().to_string();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = true;
        }
        if !in_fence {
            let leading_hashes = line
                .chars()
                .take_while(|character| *character == '#')
                .count();
            if (1..=6).contains(&leading_hashes)
                && line
                    .chars()
                    .nth(leading_hashes)
                    .is_some_and(|character| !character.is_whitespace())
            {
                line.insert(leading_hashes, ' ');
            }
        }
        if line.trim().is_empty() {
            blank_lines += 1;
            if blank_lines > 1 {
                continue;
            }
            output.push(String::new());
        } else {
            blank_lines = 0;
            output.push(line);
        }
    }
    while output.last().is_some_and(String::is_empty) {
        output.pop();
    }
    format!("{}\n", output.join("\n"))
}

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn tool(name: &str, description: &str, input_schema: Value, read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": { "type": "object", "additionalProperties": true },
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": false,
            "idempotentHint": read_only,
            "openWorldHint": false
        }
    })
}

fn tool_definitions() -> Vec<Value> {
    let text = || json!({ "type": "string" });
    let nullable_text = || json!({ "type": ["string", "null"] });
    let integer = || json!({ "type": "integer" });
    vec![
        tool("list_notebooks", "List the notebook tree and IDs.", schema(json!({}), &[]), true),
        tool("create_notebook", "Create a notebook or nested notebook.", schema(json!({ "name": text(), "parent_id": nullable_text(), "sort_order": integer() }), &["name"]), false),
        tool("list_memos", "List memo summaries, optionally filtering by notebook or query.", schema(json!({ "notebook_id": text(), "query": text(), "trash": {"type":"boolean"}, "limit": integer(), "cursor": text() }), &[]), true),
        tool("search_memos", "Search memo titles, excerpts, tags, and indexed Markdown. Use before creating potentially duplicate content.", schema(json!({ "query": text(), "notebook_id": text(), "limit": integer(), "cursor": text() }), &["query"]), true),
        tool("get_memo", "Read a memo summary, current etag, and full Markdown.", schema(json!({ "id": text() }), &["id"]), true),
        tool("create_memo", "Create a Markdown memo. Defaults to Inbox when notebook_id is omitted.", schema(json!({ "title": text(), "markdown": text(), "notebook_id": text(), "tags": {"type":"array","items":{"type":"string"}} }), &["markdown"]), false),
        tool("update_memo", "Replace a memo's Markdown with optimistic concurrency. Read it first, pass its etag, and reuse idempotency_key when retrying.", schema(json!({ "id": text(), "markdown": text(), "etag": text(), "title": text(), "tags": {"type":"array","items":{"type":"string"}}, "idempotency_key": text() }), &["id","markdown","etag"]), false),
        tool("append_to_memo", "Append focused content, optionally under a level-2 heading. Uses the latest etag if omitted; reuse idempotency_key when retrying.", schema(json!({ "id": text(), "content": text(), "heading": text(), "separator": text(), "etag": text(), "idempotency_key": text() }), &["id","content"]), false),
        tool("replace_in_memo", "Perform an exact text replacement. Multiple matches require replace_all=true.", schema(json!({ "id": text(), "find": text(), "replacement": text(), "replace_all": {"type":"boolean"}, "etag": text(), "idempotency_key": text() }), &["id","find","replacement"]), false),
        tool("format_memo", "Conservatively normalize Markdown outside fenced code: newlines, trailing whitespace, heading spacing, repeated blank lines, and final newline.", schema(json!({ "id": text(), "etag": text(), "idempotency_key": text() }), &["id"]), false),
        tool("list_todos", "List Todos using all, inbox, today, upcoming, completed, or trash filters.", schema(json!({ "filter": {"type":"string","enum":["all","inbox","today","upcoming","completed","trash"]}, "limit": integer(), "cursor": text() }), &[]), true),
        tool("get_todo", "Read one Todo including revision and etag.", schema(json!({ "id": text() }), &["id"]), true),
        tool("create_todo", "Create an independent Todo.", schema(json!({ "title": text(), "note": text(), "status": {"type":"string","enum":["inbox","open","completed"]}, "due_date": text(), "priority": {"type":"integer","minimum":0,"maximum":3}, "linked_memo_id": text() }), &["title"]), false),
        tool("update_todo", "Update Todo fields with revision/etag concurrency; null clears due_date or linked_memo_id.", schema(json!({ "id": text(), "title": text(), "note": text(), "status": {"type":"string","enum":["inbox","open","completed"]}, "due_date": nullable_text(), "priority": {"type":"integer","minimum":0,"maximum":3}, "linked_memo_id": nullable_text(), "revision": integer(), "etag": text() }), &["id"]), false),
        tool("set_todo_completed", "Complete or restore a Todo.", schema(json!({ "id": text(), "completed": {"type":"boolean"} }), &["id"]), false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync::LocalNotifyBus;

    #[test]
    fn markdown_formatter_is_conservative_and_stable() {
        let input = "#Title  \r\n\r\n\r\ntext  \r\n```rust\r\n#code  \r\n```\r\n";
        let expected = "# Title\n\ntext\n```rust\n#code  \n```\n";
        assert_eq!(format_markdown(input), expected);
        assert_eq!(format_markdown(expected), expected);
    }

    #[test]
    fn tool_catalog_contains_agent_editing_and_todo_tools() {
        let names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        assert!(names.contains(&"append_to_memo".to_string()));
        assert!(names.contains(&"format_memo".to_string()));
        assert!(names.contains(&"create_todo".to_string()));
    }

    #[tokio::test]
    async fn agent_can_create_append_without_retry_duplicates_and_manage_todos() {
        let root = std::env::temp_dir().join(format!("panda-mcp-{}", uuid::Uuid::new_v4()));
        let database = root.join("panda.db");
        std::fs::create_dir_all(&root).expect("create test root");
        let store = Store::connect(&database.to_string_lossy(), 1024 * 1024)
            .await
            .expect("connect store");
        let blobs = BlobStore::new(root.join("blobs"));
        blobs.ensure_root().await.expect("create blob root");
        let auth = AuthService::new(store.clone(), 1, 8192, 1, 1);
        auth.ensure_bootstrap("agent", "test-password")
            .await
            .expect("bootstrap auth");
        let (_, token, _, _) = auth
            .login("agent", "test-password", Some("mcp-test"), None)
            .await
            .expect("login");
        let ctx = auth
            .authenticate_bearer(&token)
            .await
            .expect("authenticate");
        let bus = Arc::new(LocalNotifyBus::new());
        let mut hints = bus.subscribe(&ctx.workspace_id).await;
        let handler = McpHandler { store, blobs, bus };

        let created = call_tool(
            &handler,
            &ctx,
            1,
            "create_memo",
            json!({ "title": "Agent notes", "markdown": "#Start\n\nBody\n" }),
        )
        .await;
        assert_eq!(
            created.pointer("/result/isError"),
            Some(&Value::Bool(false))
        );
        let memo_id = created
            .pointer("/result/structuredContent/summary/id")
            .and_then(Value::as_str)
            .expect("memo id")
            .to_string();
        let original_etag = created
            .pointer("/result/structuredContent/summary/etag")
            .and_then(Value::as_str)
            .expect("memo etag")
            .to_string();
        let hint = tokio::time::timeout(std::time::Duration::from_secs(1), hints.recv())
            .await
            .expect("sync hint timeout")
            .expect("sync hint");
        assert!(hint.kinds.contains(&"memo".to_string()));

        let append_args = json!({
            "id": memo_id,
            "heading": "Progress",
            "content": "Implemented MCP.",
            "idempotency_key": "append-progress-1"
        });
        call_tool(&handler, &ctx, 2, "append_to_memo", append_args.clone()).await;
        call_tool(&handler, &ctx, 3, "append_to_memo", append_args).await;
        let memo = call_tool(&handler, &ctx, 4, "get_memo", json!({ "id": memo_id })).await;
        let markdown = memo
            .pointer("/result/structuredContent/markdown")
            .and_then(Value::as_str)
            .expect("memo markdown");
        assert_eq!(markdown.matches("Implemented MCP.").count(), 1);

        let stale_write = call_tool(
            &handler,
            &ctx,
            8,
            "update_memo",
            json!({
                "id": memo_id,
                "etag": original_etag,
                "markdown": "This must not overwrite the append."
            }),
        )
        .await;
        assert_eq!(
            stale_write.pointer("/result/isError"),
            Some(&Value::Bool(true))
        );
        assert!(stale_write
            .pointer("/result/structuredContent/error/current_etag")
            .and_then(Value::as_str)
            .is_some());

        let todo = call_tool(
            &handler,
            &ctx,
            5,
            "create_todo",
            json!({ "title": "Review MCP", "priority": 2 }),
        )
        .await;
        let todo_id = todo
            .pointer("/result/structuredContent/id")
            .and_then(Value::as_str)
            .expect("todo id");
        let completed = call_tool(
            &handler,
            &ctx,
            6,
            "set_todo_completed",
            json!({ "id": todo_id, "completed": true }),
        )
        .await;
        assert_eq!(
            completed.pointer("/result/structuredContent/status"),
            Some(&Value::String("completed".into()))
        );

        let notification = handler
            .handle(&ctx, json!({ "jsonrpc": "2.0", "method": "ping" }))
            .await;
        assert!(notification.is_null());

        let empty_batch = handler.handle(&ctx, json!([])).await;
        assert_eq!(empty_batch.pointer("/error/code"), Some(&json!(-32600)));

        let mut read_only_ctx = ctx.clone();
        read_only_ctx.scopes = Some(vec!["memos:read".into(), "notebooks:read".into()]);
        let denied = call_tool(
            &handler,
            &read_only_ctx,
            7,
            "create_todo",
            json!({ "title": "Must not be created" }),
        )
        .await;
        assert_eq!(denied.pointer("/result/isError"), Some(&Value::Bool(true)));
        assert_eq!(
            denied.pointer("/result/structuredContent/error/code"),
            Some(&Value::String("permission_denied".into()))
        );
    }

    async fn call_tool(
        handler: &McpHandler,
        ctx: &AuthContext,
        id: i64,
        name: &str,
        arguments: Value,
    ) -> Value {
        handler
            .handle(
                ctx,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": { "name": name, "arguments": arguments }
                }),
            )
            .await
    }
}

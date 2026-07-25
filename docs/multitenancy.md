# Workspace isolation

Panda treats `workspace_id` from the authenticated session or API token as the
tenant boundary. Request bodies cannot override it.

## Authentication

- Password login accepts an optional `workspace_id`.
- Omitting it preserves the original behavior and enters the user's first
  workspace.
- A requested workspace is accepted only when `workspace_members` contains the
  user.
- `GET /api/v1/workspaces` lists all memberships for a password session. An API
  token only sees the workspace to which that token is bound.
- Owner status comes from `workspace_members.role`, not the user's global
  bootstrap-owner flag.
- Removing a workspace membership immediately invalidates sessions and API
  tokens for that user/workspace pair.

API token creation, listing, and revocation require a password session whose
role in the current workspace is `owner`. API tokens cannot mint additional
tokens.

## Data boundaries

Memo bodies, notebooks, Todos, resources, revisions, sync changes, device
cursors, and MCP tools resolve all IDs inside the authenticated workspace.
Cross-workspace notebook moves, linked Memo IDs, attachment links, and direct
Memo-body reads fail as not found.

Blob bytes remain globally content-addressed and deduplicated by hash. A
workspace can download a hash only while one of its Memos or live resources
references that hash. The `content_blobs.workspace_id` column is legacy
bookkeeping and is not used as an authorization decision.

## Current control-plane limitation

The schema supports many workspaces and memberships, and authentication can
select among them, but this server does not yet expose self-service workspace
creation, user registration, invitations, or membership administration.
Provision those records administratively until that control plane is added.

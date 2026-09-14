#!/usr/bin/env bash
set -euo pipefail

BASE="${BASE:-http://127.0.0.1:8787/api/v1}"
USER_NAME="${USER_NAME:-panda}"
PASSWORD="${PASSWORD:-PandaNote2026!}"

post_json() {
  local path="$1"
  local body="$2"
  local token="${3:-}"
  local args=(-sS -X POST "$BASE$path" -H "Content-Type: application/json")
  if [[ -n "$token" ]]; then
    args+=(-H "Authorization: Bearer $token")
  fi
  args+=(-d "$body")
  curl "${args[@]}"
}

if [[ -f /root/panda/config.yml ]]; then
  sed -i 's/allow_registration: false/allow_registration: true/' /root/panda/config.yml
  grep allow_registration /root/panda/config.yml
  systemctl restart panda.service
  systemctl is-active panda.service
  sleep 1
fi

curl -fsS "$BASE/health" >/dev/null

REG_CODE=$(curl -sS -o /tmp/panda-reg.json -w '%{http_code}' -X POST "$BASE/auth/register" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$USER_NAME\",\"password\":\"$PASSWORD\",\"device_id\":\"seed-demo\"}")
if [[ "$REG_CODE" == "200" ]]; then
  SESSION=$(python3 -c 'import json; print(json.load(open("/tmp/panda-reg.json"))["session_token"])')
  echo "registered new user: $USER_NAME"
else
  LOGIN_CODE=$(curl -sS -o /tmp/panda-login.json -w '%{http_code}' -X POST "$BASE/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$USER_NAME\",\"password\":\"$PASSWORD\",\"device_id\":\"seed-demo\"}")
  if [[ "$LOGIN_CODE" != "200" ]]; then
    echo "register failed ($REG_CODE): $(cat /tmp/panda-reg.json)"
    echo "login failed ($LOGIN_CODE): $(cat /tmp/panda-login.json)"
    exit 1
  fi
  SESSION=$(python3 -c 'import json; print(json.load(open("/tmp/panda-login.json"))["session_token"])')
  echo "logged in existing user: $USER_NAME"
fi

TOKEN_RESP=$(post_json /api-tokens '{"name":"desktop-screenshot","scopes":["memos:read","memos:write","notebooks:read","notebooks:write","resources:read","resources:write"]}' "$SESSION")
TOKEN=$(python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])' <<<"$TOKEN_RESP")
echo "API_TOKEN=$TOKEN"

create_notebook() {
  local body="$1"
  post_json /notebooks "$body" "$TOKEN"
}

WORK_ID=$(create_notebook '{"name":"Work"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
PERSONAL_ID=$(create_notebook '{"name":"Personal"}' | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
PROJECTS_ID=$(create_notebook "{\"name\":\"Projects\",\"parent_id\":\"$WORK_ID\"}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
MEETINGS_ID=$(create_notebook "{\"name\":\"Meetings\",\"parent_id\":\"$WORK_ID\"}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
JOURNAL_ID=$(create_notebook "{\"name\":\"Journal\",\"parent_id\":\"$PERSONAL_ID\"}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
READING_ID=$(create_notebook "{\"name\":\"Reading\",\"parent_id\":\"$PERSONAL_ID\"}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')

create_memo() {
  local notebook_id="$1"
  local title="$2"
  local tags_csv="$3"
  local markdown="$4"
  local pinned="${5:-0}"

  python3 - "$notebook_id" "$title" "$tags_csv" "$markdown" "$pinned" "$TOKEN" "$BASE" <<'PY'
import json, sys, urllib.request, uuid

notebook_id, title, tags_csv, markdown, pinned, token, base = sys.argv[1:8]
tags = [t for t in tags_csv.split(",") if t]

def req(method, path, payload=None):
    data = None if payload is None else json.dumps(payload).encode()
    r = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
    )
    with urllib.request.urlopen(r) as resp:
        return json.load(resp)

created = req("POST", "/memos", {
    "notebook_id": notebook_id,
    "title": title,
    "tags": tags,
    "markdown": markdown,
})
memo_id = created["summary"]["id"] if "summary" in created and created["summary"] else created.get("id")
if not memo_id and "summary" in created:
    memo_id = created["summary"]["id"]
# MemoDetail usually has summary
if isinstance(created.get("summary"), dict):
    memo_id = created["summary"]["id"]
    etag = created["summary"].get("etag")
else:
    memo_id = created["id"]
    etag = created.get("etag")

if pinned == "1":
    opened = req("POST", f"/memos/{memo_id}/open", {"acquire_lease": False})
    etag = opened.get("etag") or (opened.get("summary") or {}).get("etag") or etag
    body_md = markdown
    req("POST", f"/memos/{memo_id}/save", {
        "if_match_etag": etag,
        "idempotency_key": str(uuid.uuid4()),
        "markdown_full": body_md,
        "title": title,
        "tags": tags,
        "is_pinned": True,
    })

print(f"created note: {title}")
PY
}

create_memo "$PROJECTS_ID" "Product roadmap Q3" "product,planning,roadmap" '# Product roadmap Q3

## Themes
- Faster sync across desktop and mobile
- Cleaner capture flow for quick notes
- Better search with tags and folders

## Milestones
1. Inventory sync polish
2. Screenshot-ready sample workspace
3. Public registration preview

## Notes
Keep the narrative product-focused and short enough for screenshots.
' 1

create_memo "$MEETINGS_ID" "Weekly sync notes" "meeting,team" '# Weekly sync notes

**Attendees:** Alex, Jordan, Sam

## Decisions
- Ship English sample content for desktop screenshots
- Keep registration gated by config flag

## Action items
- [ ] Prepare release notes
- [ ] Verify folder tree looks balanced
- [ ] Pin one note for the favorites view
' 0

create_memo "$JOURNAL_ID" "Morning pages" "journal,daily" '# Morning pages

A quiet start.

Today I want the workspace to feel calm and intentional:
- clear folders
- a few meaningful tags
- notes that look real in screenshots

Leave room for whitespace.
' 0

create_memo "$READING_ID" "Notes from Deep Work" "reading,focus,book" '# Notes from Deep Work

## Key ideas
- Attention is a scarce resource
- Shallow work expands to fill the day
- Rituals protect deep blocks

## Quote
> Clarity about what matters provides clarity about what does not.

## Takeaway
Protect two uninterrupted hours for writing and design reviews.
' 0

create_memo "$WORK_ID" "Hiring checklist" "ops,hiring" '# Hiring checklist

- [ ] Write role brief
- [ ] Publish posting
- [ ] Screen portfolios
- [ ] Schedule interviews
- [ ] Send offer packet

Tags keep ops notes easy to filter later.
' 0

create_memo "$PERSONAL_ID" "Travel packing list" "personal,travel" '# Travel packing list

## Carry-on
- Passport / ID
- Laptop + charger
- Headphones
- Notebook

## Tips
Pack light. Prefer one solid outfit palette for trip photos.
' 0

echo
echo "==== desktop screenshot credentials ===="
echo "API URL: https://lab.chenyuhang.cn/panda/api/v1"
echo "Username: $USER_NAME"
echo "Password: $PASSWORD"
echo "Token: $TOKEN"
echo "Folders: Work/{Projects,Meetings}, Personal/{Journal,Reading}"
echo "======================================="

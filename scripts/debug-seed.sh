#!/usr/bin/env bash
set -euo pipefail
TOKEN='pnda_5725ea40079cd382231a436a8e22b7c29b6e8dcbbc7a114942b526892c8f6d65'
BASE='http://127.0.0.1:8787/api/v1'
echo 'create notebook:'
curl -sS -w '\nHTTP:%{http_code}\n' -X POST "$BASE/notebooks" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"Work"}'
echo
echo 'list notebooks:'
curl -sS -w '\nHTTP:%{http_code}\n' "$BASE/notebooks" \
  -H "Authorization: Bearer $TOKEN"
echo
INBOX=$(curl -sS "$BASE/notebooks" -H "Authorization: Bearer $TOKEN" | python3 -c 'import json,sys; items=json.load(sys.stdin).get("items",[]); print(next(i["id"] for i in items if i["name"]=="Inbox"))')
echo "inbox=$INBOX"
echo 'create memo:'
curl -sS -w '\nHTTP:%{http_code}\n' -X POST "$BASE/memos" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"notebook_id\":\"$INBOX\",\"title\":\"Hello\",\"markdown\":\"# Hello\\n\\nWorld\",\"tags\":[\"demo\"]}"

#!/usr/bin/env bash
set -euo pipefail
TOKEN='pnda_5725ea40079cd382231a436a8e22b7c29b6e8dcbbc7a114942b526892c8f6d65'
BASE='http://127.0.0.1:8787/api/v1'
python3 - "$TOKEN" "$BASE" <<'PY'
import json, sys, urllib.request
token, base = sys.argv[1:3]

def req(method, path):
    r = urllib.request.Request(
        base + path,
        method=method,
        headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
    )
    with urllib.request.urlopen(r) as resp:
        body = resp.read().decode()
        return json.loads(body) if body else {}

items = req("GET", "/notebooks")["items"]
works = sorted([n for n in items if n["name"]=="Work" and not n.get("parent_id")], key=lambda n: n["created_at"])
# delete empty duplicate Work folders (no children and memo_count 0), keep the first
keep = works[0]["id"]
children = {n.get("parent_id") for n in items}
for w in works[1:]:
    if w["id"] in children:
        print("skip non-empty", w["id"])
        continue
    if w["memo_count"]:
        print("skip with memos", w["id"])
        continue
    req("DELETE", f"/notebooks/{w['id']}")
    print("deleted duplicate Work", w["id"])
print("kept Work", keep)
# verify
for n in sorted(req("GET", "/notebooks")["items"], key=lambda x: (x.get("depth",0), x["path"])):
    print(f"{'  '*n['depth']}{n['name']} memos={n['memo_count']}")
PY

#!/usr/bin/env bash
set -euo pipefail
TOKEN='pnda_5725ea40079cd382231a436a8e22b7c29b6e8dcbbc7a114942b526892c8f6d65'
BASE='http://127.0.0.1:8787/api/v1'

python3 - "$TOKEN" "$BASE" <<'PY'
import json, sys, urllib.request, uuid

token, base = sys.argv[1:3]

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
        body = resp.read().decode()
        return json.loads(body) if body else {}

notebooks = {n["name"]: n for n in req("GET", "/notebooks")["items"]}
# Prefer first unique parents; child by parent path
by_id = {n["id"]: n for n in req("GET", "/notebooks")["items"]}

def find_child(parent_name, child_name):
    parents = [n for n in by_id.values() if n["name"] == parent_name and n.get("parent_id") in (None, "")]
    # if duplicates, pick oldest by created_at
    parents.sort(key=lambda n: n["created_at"])
    parent = parents[0]
    for n in by_id.values():
        if n["name"] == child_name and n.get("parent_id") == parent["id"]:
            return n["id"]
    raise SystemExit(f"missing {parent_name}/{child_name}")

work = sorted([n for n in by_id.values() if n["name"] == "Work" and not n.get("parent_id")], key=lambda n: n["created_at"])[0]["id"]
personal = sorted([n for n in by_id.values() if n["name"] == "Personal" and not n.get("parent_id")], key=lambda n: n["created_at"])[0]["id"]
projects = find_child("Work", "Projects")
meetings = find_child("Work", "Meetings")
journal = find_child("Personal", "Journal")
reading = find_child("Personal", "Reading")

notes = [
    (projects, "Product roadmap Q3", ["product", "planning", "roadmap"], True, """# Product roadmap Q3

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
"""),
    (meetings, "Weekly sync notes", ["meeting", "team"], False, """# Weekly sync notes

**Attendees:** Alex, Jordan, Sam

## Decisions
- Ship English sample content for desktop screenshots
- Keep registration gated by config flag

## Action items
- [ ] Prepare release notes
- [ ] Verify folder tree looks balanced
- [ ] Pin one note for the favorites view
"""),
    (journal, "Morning pages", ["journal", "daily"], False, """# Morning pages

A quiet start.

Today I want the workspace to feel calm and intentional:
- clear folders
- a few meaningful tags
- notes that look real in screenshots

Leave room for whitespace.
"""),
    (reading, "Notes from Deep Work", ["reading", "focus", "book"], False, """# Notes from Deep Work

## Key ideas
- Attention is a scarce resource
- Shallow work expands to fill the day
- Rituals protect deep blocks

## Quote
> Clarity about what matters provides clarity about what does not.

## Takeaway
Protect two uninterrupted hours for writing and design reviews.
"""),
    (work, "Hiring checklist", ["ops", "hiring"], False, """# Hiring checklist

- [ ] Write role brief
- [ ] Publish posting
- [ ] Screen portfolios
- [ ] Schedule interviews
- [ ] Send offer packet

Tags keep ops notes easy to filter later.
"""),
    (personal, "Travel packing list", ["personal", "travel"], False, """# Travel packing list

## Carry-on
- Passport / ID
- Laptop + charger
- Headphones
- Notebook

## Tips
Pack light. Prefer one solid outfit palette for trip photos.
"""),
]

for notebook_id, title, tags, pinned, markdown in notes:
    created = req("POST", "/memos", {
        "protocol_version": 1,
        "notebook_id": notebook_id,
        "title": title,
        "tags": tags,
        "markdown": markdown,
    })
    summary = created["summary"]
    memo_id = summary["id"]
    if pinned:
        opened = req("POST", f"/memos/{memo_id}/open", {"protocol_version": 1, "lease_mode": "none"})
        etag = opened["etag"]
        req("POST", f"/memos/{memo_id}/save", {
            "protocol_version": 1,
            "if_match_etag": etag,
            "idempotency_key": str(uuid.uuid4()),
            "markdown_full": markdown,
            "title": title,
            "tags": tags,
            "is_pinned": True,
        })
    print(f"created note: {title}")

print("tags seeded via notes: product, planning, roadmap, meeting, team, journal, daily, reading, focus, book, ops, hiring, personal, travel")
print("done")
PY

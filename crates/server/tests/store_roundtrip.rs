#[tokio::test]
async fn store_bootstrap_and_memo_roundtrip() {
    let dir = std::env::temp_dir().join(format!("panda-store-{}", uuid::Uuid::now_v7()));
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("t.db");
    let url = format!(
        "sqlite://{}?mode=rwc",
        db.to_string_lossy().replace('\\', "/")
    );
    let store = store::Store::connect(&url, 65536).await.expect("connect");
    let auth = auth::AuthService::new(store.clone(), 30, 19456, 2, 1);
    auth.ensure_bootstrap("admin", "admin123")
        .await
        .expect("bootstrap");
    let (user, token, ws, _) = auth
        .login("admin", "admin123", Some("test-device"), None)
        .await
        .expect("login");
    assert!(!token.is_empty());
    let nb = store.notebooks().default_inbox_id(&ws).await.unwrap();
    let blobs = blob::BlobStore::new(dir.join("blobs"));
    blobs.ensure_root().await.unwrap();
    let detail = store
        .memos()
        .create(
            &ws,
            &user.id,
            &nb,
            Some("t".into()),
            "# hi\n",
            &["x".into()],
            |bytes| {
                let blobs = blobs.clone();
                let data = bytes.to_vec();
                Box::pin(async move { blobs.put(&data).await })
            },
        )
        .await
        .unwrap();
    let id = detail.summary.as_ref().unwrap().id.clone();
    let (_open, etag, _) = store
        .memos()
        .open(&ws, &id, &user.id, "soft")
        .await
        .unwrap();
    let blobs2 = blob::BlobStore::new(dir.join("blobs"));
    let ack = store
        .memos()
        .save(
            &ws,
            &user.id,
            &id,
            Some(&etag),
            None,
            None,
            None,
            "idem-test",
            Some(proto::SaveBody::MarkdownFull("# hi2\n".into())),
            None,
            None,
            None,
            None,
            None,
            |bytes| {
                let blobs = blobs2.clone();
                let data = bytes.to_vec();
                Box::pin(async move { blobs.put(&data).await })
            },
            |h| {
                let blobs = blobs2.clone();
                Box::pin(async move { blobs.get(&h).await })
            },
        )
        .await
        .unwrap();
    assert_eq!(ack.revision, 2);
    let inv = store.sync().inventory(&ws, None, 100).await.unwrap();
    assert_eq!(inv.memos.len(), 1);
}

#[tokio::test]
async fn todo_updates_are_versioned_and_deletes_sync_as_tombstones() {
    let dir = std::env::temp_dir().join(format!("panda-todo-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("t.db");
    let url = format!(
        "sqlite://{}?mode=rwc",
        db.to_string_lossy().replace('\\', "/")
    );
    let store = store::Store::connect(&url, 65536).await.unwrap();
    let auth = auth::AuthService::new(store.clone(), 30, 19456, 2, 1);
    auth.ensure_bootstrap("admin", "admin123").await.unwrap();
    let (_, _, workspace_id, _) = auth
        .login("admin", "admin123", Some("todo-test"), None)
        .await
        .unwrap();

    let created = store
        .todos()
        .create(
            &workspace_id,
            store::TodoCreate {
                title: "Ship sync".into(),
                note: "server first".into(),
                status: Some("inbox".into()),
                due_date: Some("2026-07-23".into()),
                priority: Some(2),
                linked_memo_id: None,
            },
        )
        .await
        .unwrap();
    let updated = store
        .todos()
        .update(
            &workspace_id,
            &created.id,
            store::TodoUpdate {
                title: Some(created.title.clone()),
                note: Some(created.note.clone()),
                status: Some("completed".into()),
                due_date: Some(created.due_date.clone()),
                priority: Some(created.priority),
                linked_memo_id: Some(None),
                base_revision: Some(created.revision),
                if_match_etag: Some(created.etag.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.status, "completed");
    let conflict = store
        .todos()
        .update(
            &workspace_id,
            &created.id,
            store::TodoUpdate {
                title: Some("stale".into()),
                note: None,
                status: None,
                due_date: None,
                priority: None,
                linked_memo_id: None,
                base_revision: Some(created.revision),
                if_match_etag: Some(created.etag.clone()),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.code, domain::ErrorCode::Conflict);

    store
        .todos()
        .delete(
            &workspace_id,
            &updated.id,
            Some(updated.revision),
            Some(&updated.etag),
            false,
        )
        .await
        .unwrap();
    let (changes, _, _) = store.sync().pull(&workspace_id, 0, 20).await.unwrap();
    let tombstone = changes
        .into_iter()
        .find(|change| change.entity_type == "todo")
        .unwrap();
    assert_eq!(tombstone.operation, "delete");
    let todo: store::Todo =
        serde_json::from_str(tombstone.payload_json.as_deref().unwrap()).unwrap();
    assert!(todo.is_deleted);
    assert_eq!(todo.revision, 3);
    assert!(todo.deleted_at.is_some());

    let (trashed, _) = store
        .todos()
        .list(
            &workspace_id,
            store::TodoListQuery {
                filter: Some("trash".into()),
                limit: 20,
                cursor: None,
            },
        )
        .await
        .unwrap();
    let trashed = trashed
        .into_iter()
        .find(|todo| todo.id == updated.id)
        .unwrap();
    store
        .todos()
        .delete(
            &workspace_id,
            &trashed.id,
            Some(trashed.revision),
            Some(&trashed.etag),
            true,
        )
        .await
        .unwrap();
    let (trashed, _) = store
        .todos()
        .list(
            &workspace_id,
            store::TodoListQuery {
                filter: Some("trash".into()),
                limit: 20,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert!(trashed.is_empty());
}

#[tokio::test]
async fn register_creates_isolated_personal_workspace() {
    let dir = std::env::temp_dir().join(format!("panda-register-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("t.db");
    let url = format!(
        "sqlite://{}?mode=rwc",
        db.to_string_lossy().replace('\\', "/")
    );
    let store = store::Store::connect(&url, 65536).await.unwrap();
    let auth = auth::AuthService::new(store.clone(), 30, 19456, 2, 1);
    auth.ensure_bootstrap("admin", "admin123").await.unwrap();
    let (_, _, admin_ws, _) = auth
        .login("admin", "admin123", Some("admin-device"), None)
        .await
        .unwrap();

    let short = auth
        .register("alice", "short", None)
        .await
        .unwrap_err();
    assert_eq!(short.code, domain::ErrorCode::InvalidArgument);

    let (user, token, alice_ws, _) = auth
        .register("alice", "alice-pass-ok", Some("alice-device"))
        .await
        .unwrap();
    assert!(!token.is_empty());
    assert_ne!(alice_ws, admin_ws);
    assert_eq!(user.username, "alice");
    assert_eq!(user.is_owner, 1);

    let dup = auth
        .register("alice", "another-password", None)
        .await
        .unwrap_err();
    assert_eq!(dup.code, domain::ErrorCode::Conflict);

    let alice_nbs = store.notebooks().list(&alice_ws).await.unwrap();
    assert_eq!(alice_nbs.len(), 1);
    assert_eq!(alice_nbs[0].name, "Inbox");

    let admin_nbs = store.notebooks().list(&admin_ws).await.unwrap();
    assert!(admin_nbs.len() > 1);
    assert!(!admin_nbs.iter().any(|nb| nb.id == alice_nbs[0].id));
}

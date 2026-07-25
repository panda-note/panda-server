use blob::BlobStore;
use domain::new_id;
use store::{Store, TodoCreate};

#[tokio::test]
async fn workspace_membership_and_content_boundaries_are_enforced() {
    let dir = std::env::temp_dir().join(format!("panda-tenant-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("tenant.db");
    let url = format!(
        "sqlite://{}?mode=rwc",
        db.to_string_lossy().replace('\\', "/")
    );
    let store = Store::connect(&url, 8).await.unwrap();
    let auth = auth::AuthService::new(store.clone(), 30, 8192, 1, 1);
    auth.ensure_bootstrap("admin", "admin123").await.unwrap();

    let (owner, _, workspace_a, _) = auth
        .login("admin", "admin123", Some("tenant-a"), None)
        .await
        .unwrap();
    let notebook_a = store
        .notebooks()
        .default_inbox_id(&workspace_a)
        .await
        .unwrap();

    let workspace_b = new_id();
    let notebook_b = new_id();
    let workspace_c = new_id();
    let now = store::now_rfc3339();
    sqlx::query("INSERT INTO workspaces(id,name,created_at,updated_at) VALUES(?,?,?,?),(?,?,?,?)")
        .bind(&workspace_b)
        .bind("Tenant B")
        .bind(&now)
        .bind(&now)
        .bind(&workspace_c)
        .bind("Tenant C")
        .bind(&now)
        .bind(&now)
        .execute(store.db.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO workspace_members(workspace_id,user_id,role,created_at) VALUES(?,?,'member',?)",
    )
    .bind(&workspace_b)
    .bind(&owner.id)
    .bind(&now)
    .execute(store.db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notebooks(id,workspace_id,parent_id,name,slug,path,depth,sort_order,is_deleted,created_at,updated_at)
         VALUES(?,?,NULL,'Inbox','inbox',?,0,0,0,?,?)",
    )
    .bind(&notebook_b)
    .bind(&workspace_b)
    .bind(format!("/{notebook_b}"))
    .bind(&now)
    .bind(&now)
    .execute(store.db.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO sync_meta(workspace_id,sync_epoch,updated_at) VALUES(?,1,?),(?,1,?)")
        .bind(&workspace_b)
        .bind(&now)
        .bind(&workspace_c)
        .bind(&now)
        .execute(store.db.pool())
        .await
        .unwrap();

    let (member, session_b, selected_b, _) = auth
        .login("admin", "admin123", Some("tenant-b"), Some(&workspace_b))
        .await
        .unwrap();
    assert_eq!(selected_b, workspace_b);
    assert_eq!(member.is_owner, 0);
    let context_b = auth.authenticate_bearer(&session_b).await.unwrap();
    assert_eq!(context_b.workspace_id, workspace_b);
    assert!(!context_b.is_owner);

    let unknown_workspace = new_id();
    let denied_login = auth
        .login(
            "admin",
            "admin123",
            Some("unknown"),
            Some(&unknown_workspace),
        )
        .await
        .unwrap_err();
    assert_eq!(
        denied_login.code,
        domain::ErrorCode::PermissionDenied,
        "a user must not select a workspace without membership"
    );

    let blobs = BlobStore::new(dir.join("blobs"));
    blobs.ensure_root().await.unwrap();
    let memo_a = create_memo(
        &store,
        &blobs,
        &workspace_a,
        &owner.id,
        &notebook_a,
        "Tenant A secret",
    )
    .await;
    let memo_b = create_memo(
        &store,
        &blobs,
        &workspace_b,
        &owner.id,
        &notebook_b,
        "Tenant B secret",
    )
    .await;

    let cross_read = store
        .memos()
        .read_markdown(&workspace_b, &memo_a, {
            let blobs = blobs.clone();
            move |hash| {
                let blobs = blobs.clone();
                Box::pin(async move { blobs.get(&hash).await })
            }
        })
        .await;
    assert!(cross_read.is_err());

    let wrong_notebook = store
        .memos()
        .create(
            &workspace_b,
            &owner.id,
            &notebook_a,
            Some("Wrong tenant".into()),
            "must fail",
            &[],
            {
                let blobs = blobs.clone();
                move |bytes| {
                    let blobs = blobs.clone();
                    let data = bytes.to_vec();
                    Box::pin(async move { blobs.put(&data).await })
                }
            },
        )
        .await;
    assert!(wrong_notebook.is_err());

    let cross_todo = store
        .todos()
        .create(
            &workspace_b,
            TodoCreate {
                title: "Cross tenant link".into(),
                note: String::new(),
                status: None,
                due_date: None,
                priority: None,
                linked_memo_id: Some(memo_a.clone()),
            },
        )
        .await;
    assert!(cross_todo.is_err());

    let bytes = b"shared bytes across workspaces";
    let (shared_hash, shared_size) = blobs.put(bytes).await.unwrap();
    store
        .resources()
        .create(
            &workspace_a,
            Some(&memo_a),
            &shared_hash,
            "attachment",
            None,
            Some("shared.bin"),
            shared_size,
        )
        .await
        .unwrap();
    store
        .resources()
        .create(
            &workspace_b,
            Some(&memo_b),
            &shared_hash,
            "attachment",
            None,
            Some("shared.bin"),
            shared_size,
        )
        .await
        .unwrap();
    assert!(store
        .memos()
        .can_access_blob(&workspace_a, &shared_hash)
        .await
        .unwrap());
    assert!(store
        .memos()
        .can_access_blob(&workspace_b, &shared_hash)
        .await
        .unwrap());
    assert!(!store
        .memos()
        .can_access_blob(&workspace_c, &shared_hash)
        .await
        .unwrap());

    let cross_resource = store
        .resources()
        .create(
            &workspace_b,
            Some(&memo_a),
            &shared_hash,
            "attachment",
            None,
            None,
            shared_size,
        )
        .await;
    assert!(cross_resource.is_err());

    let raw_api_token = auth::AuthService::mint_token("pnda_");
    store
        .users()
        .create_api_token(
            &workspace_b,
            &owner.id,
            "tenant-b-test",
            &auth::AuthService::hash_token(&raw_api_token),
            "[\"memos:read\"]",
            None,
        )
        .await
        .unwrap();
    let api_context = auth.authenticate_bearer(&raw_api_token).await.unwrap();
    assert_eq!(api_context.workspace_id, workspace_b);
    assert!(!api_context.is_owner);

    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = ? AND user_id = ?")
        .bind(&workspace_b)
        .bind(&owner.id)
        .execute(store.db.pool())
        .await
        .unwrap();
    assert!(
        auth.authenticate_bearer(&raw_api_token).await.is_err(),
        "removing membership must revoke effective API-token access"
    );
    assert!(
        auth.authenticate_bearer(&session_b).await.is_err(),
        "removing membership must revoke effective session access"
    );
}

async fn create_memo(
    store: &Store,
    blobs: &BlobStore,
    workspace_id: &str,
    user_id: &str,
    notebook_id: &str,
    markdown: &str,
) -> String {
    let detail = store
        .memos()
        .create(workspace_id, user_id, notebook_id, None, markdown, &[], {
            let blobs = blobs.clone();
            move |bytes| {
                let blobs = blobs.clone();
                let data = bytes.to_vec();
                Box::pin(async move { blobs.put(&data).await })
            }
        })
        .await
        .unwrap();
    detail.summary.unwrap().id
}

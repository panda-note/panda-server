//! Markdown ZIP export (notebook tree + notes).

use domain::{PandaError, PandaResult};
use std::io::{Cursor, Write};
use store::Store;

fn io_err(e: std::io::Error) -> PandaError {
    PandaError::internal(e.to_string())
}

/// Minimal ZIP (store-only, no compression) writer for export archives.
pub fn write_store_zip(files: &[(String, Vec<u8>)]) -> PandaResult<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let mut central = Vec::new();

    for (name, data) in files {
        let name_bytes = name.as_bytes();
        let local_offset = out.position() as u32;

        out.write_all(&[0x50, 0x4b, 0x03, 0x04]).map_err(io_err)?;
        out.write_all(&[0x14, 0x00]).map_err(io_err)?;
        out.write_all(&[0x00, 0x00]).map_err(io_err)?;
        out.write_all(&[0x00, 0x00]).map_err(io_err)?;
        out.write_all(&[0x00, 0x00, 0x00, 0x00]).map_err(io_err)?;
        let crc = crc32(data);
        out.write_all(&crc.to_le_bytes()).map_err(io_err)?;
        out.write_all(&(data.len() as u32).to_le_bytes())
            .map_err(io_err)?;
        out.write_all(&(data.len() as u32).to_le_bytes())
            .map_err(io_err)?;
        out.write_all(&(name_bytes.len() as u16).to_le_bytes())
            .map_err(io_err)?;
        out.write_all(&0u16.to_le_bytes()).map_err(io_err)?;
        out.write_all(name_bytes).map_err(io_err)?;
        out.write_all(data).map_err(io_err)?;

        let mut cen = Vec::new();
        cen.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        cen.extend_from_slice(&[0x14, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00]);
        cen.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        cen.extend_from_slice(&crc.to_le_bytes());
        cen.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cen.extend_from_slice(&(data.len() as u32).to_le_bytes());
        cen.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        cen.extend_from_slice(&0u16.to_le_bytes());
        cen.extend_from_slice(&0u16.to_le_bytes());
        cen.extend_from_slice(&0u16.to_le_bytes());
        cen.extend_from_slice(&0u16.to_le_bytes());
        cen.extend_from_slice(&0u32.to_le_bytes());
        cen.extend_from_slice(&local_offset.to_le_bytes());
        cen.extend_from_slice(name_bytes);
        central.push(cen);
    }

    let central_offset = out.position() as u32;
    for cen in &central {
        out.write_all(cen).map_err(io_err)?;
    }
    let central_size = out.position() as u32 - central_offset;
    out.write_all(&[0x50, 0x4b, 0x05, 0x06]).map_err(io_err)?;
    out.write_all(&0u16.to_le_bytes()).map_err(io_err)?;
    out.write_all(&0u16.to_le_bytes()).map_err(io_err)?;
    out.write_all(&(files.len() as u16).to_le_bytes())
        .map_err(io_err)?;
    out.write_all(&(files.len() as u16).to_le_bytes())
        .map_err(io_err)?;
    out.write_all(&central_size.to_le_bytes()).map_err(io_err)?;
    out.write_all(&central_offset.to_le_bytes())
        .map_err(io_err)?;
    out.write_all(&0u16.to_le_bytes()).map_err(io_err)?;
    Ok(out.into_inner())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (!(crc & 1)).wrapping_add(1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub async fn export_workspace_markdown_zip(
    store: &Store,
    blobs: &blob::BlobStore,
    workspace_id: &str,
) -> PandaResult<Vec<u8>> {
    let notebooks = store.notebooks().list(workspace_id).await?;
    let (memos, _) = store
        .memos()
        .list(
            workspace_id,
            store::MemoListQuery {
                notebook_id: None,
                trash: false,
                q: None,
                limit: 10_000,
                cursor: None,
            },
        )
        .await?;

    let mut files = Vec::new();
    files.push((
        "README.md".into(),
        format!(
            "# Panda export\n\nnotebooks: {}\nmemos: {}\n",
            notebooks.len(),
            memos.len()
        )
        .into_bytes(),
    ));

    for m in memos {
        let md = store
            .memos()
            .read_markdown(workspace_id, &m.id, {
                let blobs = blobs.clone();
                move |h| {
                    let blobs = blobs.clone();
                    Box::pin(async move { blobs.get(&h).await })
                }
            })
            .await
            .unwrap_or_default();
        let title = m.title.clone().unwrap_or_else(|| m.id.clone());
        let safe: String = title
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let front = format!(
            "---\nid: {}\nnotebook_id: {}\ntitle: {:?}\ntags: {:?}\n---\n\n",
            m.id, m.notebook_id, title, m.tags
        );
        let suffix = m.id.get(..8).unwrap_or(&m.id);
        files.push((
            format!("notes/{safe}-{suffix}.md"),
            format!("{front}{md}").into_bytes(),
        ));
    }

    write_store_zip(&files)
}

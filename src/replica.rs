use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Result, Row};
use serde::{Deserialize, Serialize};

use crate::repository::NoteRepository;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Record {
    pub key: String,
    pub title: String,
    pub content: String,
    pub folder: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Change {
    pub record: Record,
    pub revision: i64,
    #[serde(skip)]
    generation: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Remote {
    pub record: Record,
    pub revision: i64,
}

pub struct Download {
    pub sent: Vec<Change>,
    pub accepted: Vec<Remote>,
    pub current: Vec<Remote>,
}

const NOTE_RECORD: &str = "
    SELECT 'note:' || notes.sync_id, notes.title, notes.content, lower(folders.name),
           notes.created_at, notes.updated_at, notes.deleted, notes.cloud_revision,
           notes.generation, notes.synced_generation, notes.blocks
    FROM notes LEFT JOIN folders ON folders.id = notes.folder_id";

fn record(row: &Row<'_>) -> Result<Record> {
    Ok(Record {
        key: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        folder: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        deleted: row.get(6)?,
        blocks: row.get(10)?,
    })
}

impl NoteRepository {
    pub fn sync_setup(&self) -> Result<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT project, publishable_key FROM sync_setup WHERE slot = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
    }

    pub fn save_sync_setup(&self, project: &str, key: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_setup (slot, project, publishable_key) VALUES (1, ?1, ?2)
             ON CONFLICT(slot) DO UPDATE SET project = excluded.project, publishable_key = excluded.publishable_key",
            (project, key),
        )?;
        Ok(())
    }

    pub fn claim_workspace(&self, owner: &str) -> Result<bool> {
        self.conn.execute(
            "INSERT INTO sync_account (slot, owner) VALUES (1, ?1) ON CONFLICT(slot) DO NOTHING",
            [owner],
        )?;
        self.conn.query_row(
            "SELECT owner = ?1 FROM sync_account WHERE slot = 1",
            [owner],
            |row| row.get(0),
        )
    }

    pub fn sync_changes(&self) -> Result<Vec<Change>> {
        let mut folders = self.conn.prepare(
            "SELECT 'folder:' || lower(name), name, '', NULL, '', '', 0, 0, 0, 0, NULL
             FROM folders WHERE cloud_revision = 0 ORDER BY id",
        )?;
        let folders = folders.query_map([], |row| {
            Ok(Change {
                record: record(row)?,
                revision: 0,
                generation: 0,
            })
        })?;
        let mut notes = self.conn.prepare(&format!(
            "{NOTE_RECORD} WHERE generation > synced_generation ORDER BY notes.id"
        ))?;
        let notes = notes.query_map([], |row| {
            Ok(Change {
                record: record(row)?,
                revision: row.get(7)?,
                generation: row.get(8)?,
            })
        })?;
        folders.chain(notes).collect()
    }

    pub fn apply_sync(&self, download: Download) -> Result<usize> {
        let transaction = self.conn.unchecked_transaction()?;
        let mut conflicts = 0;
        if download.sent.len() != download.accepted.len() {
            return Err(rusqlite::Error::InvalidQuery);
        }

        for remote in download
            .current
            .iter()
            .filter(|entry| entry.record.key.starts_with("folder:"))
        {
            transaction.execute(
                "INSERT INTO folders (name, cloud_revision) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET cloud_revision = max(cloud_revision, excluded.cloud_revision)",
                (&remote.record.title, remote.revision),
            )?;
        }

        for (sent, remote) in download.sent.iter().zip(&download.accepted) {
            if sent.record.key != remote.record.key {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let Some(sync_id) = remote.record.key.strip_prefix("note:") else {
                continue;
            };
            let Some((local, revision, _, _)) = read_local(&transaction, sync_id)? else {
                continue;
            };
            if revision > remote.revision
                || (revision == remote.revision && sent.record != remote.record)
            {
                continue;
            }
            if sent.record == remote.record {
                transaction.execute(
                    "UPDATE notes SET cloud_revision = ?1, synced_generation = ?2 WHERE sync_id = ?3",
                    (remote.revision, sent.generation, sync_id),
                )?;
            } else if local.deleted && !remote.record.deleted {
                keep_copy(&transaction, &remote.record)?;
                conflicts += 1;
                transaction.execute(
                    "UPDATE notes SET cloud_revision = ?1 WHERE sync_id = ?2",
                    (remote.revision, sync_id),
                )?;
            } else {
                if local != remote.record && !local.deleted {
                    keep_copy(&transaction, &local)?;
                    conflicts += 1;
                }
                write_remote(&transaction, sync_id, remote)?;
            }
        }

        for remote in &download.current {
            let Some(sync_id) = remote.record.key.strip_prefix("note:") else {
                continue;
            };
            if let Some((_, revision, generation, synced)) = read_local(&transaction, sync_id)? {
                if revision >= remote.revision || generation != synced {
                    continue;
                }
            }
            write_remote(&transaction, sync_id, remote)?;
        }

        transaction.commit()?;
        Ok(conflicts)
    }
}

fn read_local(conn: &Connection, sync_id: &str) -> Result<Option<(Record, i64, i64, i64)>> {
    conn.query_row(
        &format!("{NOTE_RECORD} WHERE notes.sync_id = ?1"),
        [sync_id],
        |row| Ok((record(row)?, row.get(7)?, row.get(8)?, row.get(9)?)),
    )
    .optional()
}

fn keep_copy(conn: &Connection, note: &Record) -> Result<()> {
    let title = if note.title.trim().is_empty() {
        "Untitled"
    } else {
        &note.title
    };
    conn.execute(
        "INSERT INTO notes (sync_id, title, content, folder_id, created_at, updated_at, blocks)
         VALUES (lower(hex(randomblob(16))), ?1, ?2,
                 (SELECT id FROM folders WHERE name = ?3), ?4, ?4, ?5)",
        (
            format!("{title} (conflict copy)"),
            &note.content,
            &note.folder,
            Utc::now().to_rfc3339(),
            &note.blocks,
        ),
    )?;
    Ok(())
}

fn write_remote(conn: &Connection, sync_id: &str, remote: &Remote) -> Result<()> {
    let note = &remote.record;
    conn.execute(
        "INSERT INTO notes (sync_id, title, content, folder_id, created_at, updated_at, deleted, cloud_revision, synced_generation, blocks)
         VALUES (?1, ?2, ?3, (SELECT id FROM folders WHERE name = ?4), ?5, ?6, ?7, ?8, 1, ?9)
         ON CONFLICT(sync_id) DO UPDATE SET title = excluded.title, content = excluded.content, blocks = excluded.blocks,
             folder_id = excluded.folder_id, created_at = excluded.created_at, updated_at = excluded.updated_at,
             deleted = excluded.deleted, cloud_revision = excluded.cloud_revision, synced_generation = notes.generation",
        (sync_id, &note.title, &note.content, &note.folder, &note.created_at, &note.updated_at, note.deleted, remote.revision, &note.blocks),
    )?;
    Ok(())
}

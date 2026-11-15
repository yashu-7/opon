use std::{fs, path::Path};

use rusqlite::{Connection, TransactionBehavior};

pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub blocks: Option<String>,
    pub folder_id: Option<i64>,
}

pub fn initialize(directory: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    fs::create_dir_all(directory)?;
    let mut conn = Connection::open(directory.join("opon.db"))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let version: u32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > 5 {
        return Err(std::io::Error::other("This database needs a newer version of OPON").into());
    }
    if version == 0 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             CREATE TABLE folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL COLLATE NOCASE UNIQUE
             );
             ALTER TABLE notes ADD COLUMN folder_id INTEGER REFERENCES folders(id);",
        )?;
    }
    if version < 2 {
        transaction.execute_batch(
            "ALTER TABLE notes ADD COLUMN sync_id TEXT;
             UPDATE notes SET sync_id = lower(hex(randomblob(16)));
             CREATE UNIQUE INDEX note_sync_id ON notes(sync_id);
             ALTER TABLE notes ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE notes ADD COLUMN generation INTEGER NOT NULL DEFAULT 1;
             ALTER TABLE notes ADD COLUMN synced_generation INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE notes ADD COLUMN cloud_revision INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE folders ADD COLUMN cloud_revision INTEGER NOT NULL DEFAULT 0;
             CREATE TABLE sync_account (
                slot INTEGER PRIMARY KEY CHECK (slot = 1),
                owner TEXT NOT NULL
             );
             PRAGMA user_version = 2;",
        )?;
    }
    if version < 3 {
        transaction.execute_batch(
            "CREATE TABLE sync_setup (
                 slot INTEGER PRIMARY KEY CHECK (slot = 1),
                 project TEXT NOT NULL,
                 publishable_key TEXT NOT NULL
             );
             PRAGMA user_version = 3;",
        )?;
    }
    if version < 4 {
        transaction.execute_batch(
            "ALTER TABLE notes ADD COLUMN blocks TEXT;
             PRAGMA user_version = 4;",
        )?;
    }
    if version < 5 {
        transaction.execute_batch(
            "CREATE TABLE preferences (
                 name TEXT PRIMARY KEY NOT NULL,
                 value TEXT NOT NULL
             );
             PRAGMA user_version = 5;",
        )?;
    }
    transaction.commit()?;

    Ok(conn)
}

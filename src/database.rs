use std::fs;
use rusqlite::{Connection, Result};

#[derive(Debug, Clone)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

pub fn initialize() -> Result<Connection, Box<dyn std::error::Error>> {
    let db_dir = "data";
    let db_file_path = "data/opon.db";

    fs::create_dir_all(db_dir)?;

    let conn = Connection::open(db_file_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )?;

    Ok(conn)
}
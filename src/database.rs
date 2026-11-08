use chrono;
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

pub fn initialize() -> Result<Connection> {
    let db_dir = "data";
    let db_file_path = "data/opon.db";

    fs::create_dir_all(db_dir)
        .expect("Failed to create database directory");

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

pub fn add_note(conn: &Connection, title: &str, content: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc2822();

    conn.execute("INSERT INTO notes (title, content, created_at, updated_at)
                    VALUES (?1, ?2, ?3, ?4)",
                    (title, content, &now, &now),)?;  

    Ok(())
}
use chrono::Utc;
use rusqlite::{Connection, Result};

use crate::database::Note;

pub struct NoteRepository {
    conn: Connection,
}

impl NoteRepository {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn add_note(
        &self,
        title: &str,
        content: &str,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT INTO notes (title, content, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            (title, content, &now, &now),
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_notes(&self) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, content, created_at, updated_at
             FROM notes
             ORDER BY updated_at DESC",
        )?;

        let notes = stmt.query_map([], |row| {
            Ok(Note {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;

        let mut results = Vec::new();

        for note in notes {
            results.push(note?);
        }

        Ok(results)
    }

    pub fn update_note(
        &self,
        id: i64,
        title: &str,
        content: &str,
    ) -> Result<usize> {
        let now = Utc::now().to_rfc3339();

        let rows_affected = self.conn.execute(
            "UPDATE notes
             SET title = ?1,
                 content = ?2,
                 updated_at = ?3
             WHERE id = ?4",
            (title, content, &now, id),
        )?;

        Ok(rows_affected)
    }

    pub fn delete_note(&self, id: i64) -> Result<usize> {
        let rows_affected = self.conn.execute(
            "DELETE FROM notes WHERE id = ?1",
            [id],
        )?;

        Ok(rows_affected)
    }
}
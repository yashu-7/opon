use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Result, Row};

use crate::database::Note;

pub struct NoteRepository {
    pub(crate) conn: Connection,
}

pub struct Folder {
    pub id: i64,
    pub name: String,
}

fn read_note(row: &Row<'_>) -> Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        title: row.get(1)?,
        content: row.get(2)?,
        folder_id: row.get(3)?,
        blocks: row.get(4)?,
    })
}

impl NoteRepository {
    pub fn preference(&self, name: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM preferences WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn set_preference(&self, name: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO preferences VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value",
            (name, value),
        )?;
        Ok(())
    }

    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    pub fn add_note(&self, title: &str, content: &str, folder_id: Option<i64>) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO notes (title, content, created_at, updated_at, folder_id, sync_id)
             VALUES (?1, ?2, ?3, ?3, ?4, lower(hex(randomblob(16))))",
            (title, content, &now, folder_id),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_notes(&self, folder_id: Option<i64>, search: &str) -> Result<Vec<(i64, String)>> {
        let search = search.trim().to_lowercase();
        let mut stmt = self.conn.prepare(
            "SELECT id, title, CASE WHEN ?2 THEN content ELSE '' END FROM notes
             WHERE deleted = 0 AND (?1 IS NULL OR folder_id = ?1)
             ORDER BY updated_at DESC, id DESC",
        )?;
        let mut rows = stmt.query((folder_id, !search.is_empty()))?;
        let mut notes = Vec::new();
        while let Some(row) = rows.next()? {
            let title: String = row.get(1)?;
            if search.is_empty()
                || title.to_lowercase().contains(&search)
                || row.get::<_, String>(2)?.to_lowercase().contains(&search)
            {
                notes.push((row.get(0)?, title));
            }
        }
        Ok(notes)
    }

    pub fn get_note_by_id(&self, id: i64) -> Result<Option<Note>> {
        self.conn
            .query_row(
                "SELECT id, title, content, folder_id, blocks FROM notes WHERE id = ?1 AND deleted = 0",
                [id],
                read_note,
            )
            .optional()
    }

    pub fn update_note(
        &self,
        id: i64,
        title: &str,
        content: &str,
        blocks: Option<&str>,
    ) -> Result<usize> {
        self.conn.execute(
            "UPDATE notes SET title = ?1, content = ?2, blocks = ?3,
             updated_at = ?4, generation = generation + 1 WHERE id = ?5 AND deleted = 0",
            (title, content, blocks, Utc::now().to_rfc3339(), id),
        )
    }

    pub fn delete_note(&self, id: i64) -> Result<usize> {
        self.conn.execute(
            "UPDATE notes SET title = '', content = '', blocks = NULL, folder_id = NULL, deleted = 1,
             updated_at = ?1, generation = generation + 1 WHERE id = ?2 AND deleted = 0",
            (Utc::now().to_rfc3339(), id),
        )
    }

    pub fn add_folder(&self, name: &str) -> Result<()> {
        self.conn
            .execute("INSERT INTO folders (name) VALUES (?1)", [name])?;
        Ok(())
    }

    pub fn get_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM folders ORDER BY name, id")?;
        let folders = stmt.query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        folders.collect()
    }

    pub fn move_note(&self, id: i64, folder_id: Option<i64>) -> Result<usize> {
        self.conn.execute(
            "UPDATE notes SET folder_id = ?1, updated_at = ?2, generation = generation + 1
             WHERE id = ?3 AND deleted = 0",
            (folder_id, Utc::now().to_rfc3339(), id),
        )
    }
}

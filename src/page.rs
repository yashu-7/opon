use std::{cell::RefCell, rc::Rc};

use rusqlite::{Error, Result};
use serde::{Deserialize, Serialize};
use slint::{Model, ModelRc, SharedString, VecModel};
use uuid::Uuid;

use crate::{database::Note, BlockAction, BlockChoice, BlockRowData, MainWindow};

pub fn choices(search: SharedString) -> ModelRc<BlockChoice> {
    let search = search.trim().to_lowercase();
    let kinds = [
        ("text", "Text", "Aa", "paragraph"),
        ("heading", "Heading 1", "H1", "h1"),
        ("subheading", "Heading 2", "H2", "h2"),
        ("bullet", "Bulleted list", "•", "bullet"),
        ("number", "Numbered list", "1.", "ordered"),
        ("todo", "To-do list", "☑", "checkbox checklist task todo"),
        ("quote", "Quote", "\"", "callout"),
        ("code", "Code", "</>", "snippet"),
        ("divider", "Divider", "—", "separator"),
    ];
    Rc::new(VecModel::from_iter(kinds.into_iter().filter_map(
        |(kind, label, glyph, aliases)| {
            (label.to_lowercase().contains(&search) || aliases.contains(&search)).then(|| {
                BlockChoice {
                    kind: kind.into(),
                    label: label.into(),
                    glyph: glyph.into(),
                }
            })
        },
    )))
    .into()
}

#[derive(Serialize, Deserialize)]
struct Block {
    id: String,
    kind: String,
    text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    checked: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl From<BlockRowData> for Block {
    fn from(row: BlockRowData) -> Self {
        Self {
            id: row.id.to_string(),
            kind: row.kind.to_string(),
            text: row.text.to_string(),
            checked: row.checked,
        }
    }
}

impl From<Block> for BlockRowData {
    fn from(block: Block) -> Self {
        Self {
            id: block.id.into(),
            kind: block.kind.into(),
            text: block.text.into(),
            checked: block.checked,
            number: 0,
        }
    }
}

#[derive(Default)]
pub struct Page {
    pub rows: Rc<VecModel<BlockRowData>>,
    saved: RefCell<Option<(String, Option<String>)>>,
}

impl Page {
    pub fn open(&self, note: &Note, switching: bool) {
        let unchanged = self
            .saved
            .borrow()
            .as_ref()
            .is_some_and(|(text, blocks)| *text == note.content && *blocks == note.blocks);
        if !switching && unchanged {
            return;
        }
        let rows: Vec<_> = note
            .blocks
            .as_deref()
            .and_then(|json| serde_json::from_str::<Vec<Block>>(json).ok())
            .filter(|blocks| !blocks.is_empty())
            .map(|blocks| blocks.into_iter().map(BlockRowData::from).collect())
            .unwrap_or_else(|| {
                note.content
                    .split('\n')
                    .map(|line| new_row("text", line))
                    .collect()
            });
        self.rows.set_vec(rows);
        self.number_lists();
        self.mark_saved(note.content.clone(), note.blocks.clone());
    }

    pub fn clear(&self) {
        self.rows.set_vec(Vec::new());
        self.saved.replace(None);
    }

    pub fn encode(&self) -> Result<(String, String)> {
        let blocks: Vec<_> = self.rows.iter().map(Block::from).collect();
        let plain = blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let json = serde_json::to_string(&blocks)
            .map_err(|error| Error::ToSqlConversionFailure(Box::new(error)))?;
        Ok((plain, json))
    }

    pub fn mark_saved(&self, plain: String, blocks: Option<String>) {
        self.saved.replace(Some((plain, blocks)));
    }

    pub fn focus_first(&self, window: &MainWindow) {
        self.focus(window, 0, false);
    }

    // Returns whether the command changed the saved page.
    pub fn apply(
        &self,
        window: &MainWindow,
        command: BlockAction,
        index: i32,
        value: &str,
        anchor: i32,
        cursor: i32,
    ) -> bool {
        let Ok(index) = usize::try_from(index) else {
            return false;
        };
        let Some(mut row) = self.rows.row_data(index) else {
            return false;
        };
        let count = self.rows.row_count();

        match command {
            BlockAction::Edit => {
                let shortcut = match value {
                    "# " => Some("heading"),
                    "## " => Some("subheading"),
                    "- " | "* " => Some("bullet"),
                    "1. " => Some("number"),
                    "[] " | "[ ] " => Some("todo"),
                    "> " => Some("quote"),
                    "```" => Some("code"),
                    _ => None,
                };
                if row.kind == "text" {
                    if let Some(kind) = shortcut {
                        row.kind = kind.into();
                        row.text = "".into();
                        self.rows.set_row_data(index, row);
                        self.number_lists();
                        return true;
                    }
                }
                if row.text == value {
                    return false;
                }
                row.text = value.into();
                self.rows.set_row_data(index, row);
                return true;
            }
            BlockAction::Split => {
                let (head, tail) = split_selection(row.text.as_str(), anchor, cursor);
                let continuation = match row.kind.as_str() {
                    "bullet" | "number" | "todo" if head.is_empty() && tail.is_empty() => {
                        row.kind = "text".into();
                        self.rows.set_row_data(index, row);
                        self.number_lists();
                        return true;
                    }
                    "bullet" | "number" | "todo" => row.kind.clone(),
                    _ => "text".into(),
                };
                let next = new_row(&continuation, tail);
                row.text = head.into();
                self.rows.set_row_data(index, row);
                self.rows.insert(index + 1, next);
                self.focus(window, index + 1, false);
            }
            BlockAction::Insert => {
                self.rows.insert(index + 1, new_row("text", ""));
                self.focus(window, index + 1, false);
            }
            BlockAction::Join => {
                let Some(previous_index) = index.checked_sub(1) else {
                    return false;
                };
                let Some(mut previous) = self.rows.row_data(previous_index) else {
                    return false;
                };
                if previous.kind == "divider" {
                    self.rows.remove(previous_index);
                    self.focus(window, previous_index, false);
                } else {
                    let boundary = previous.text.len() as i32;
                    previous.text = format!("{}{}", previous.text, row.text).into();
                    self.rows.remove(index);
                    self.rows.set_row_data(previous_index, previous);
                    self.focus(window, previous_index, false);
                    window.set_focus_offset(boundary);
                }
            }
            BlockAction::Convert => {
                if row.kind == value {
                    self.focus(window, index, true);
                    return false;
                }
                if value == "divider" {
                    if row.text.is_empty() {
                        row.kind = value.into();
                        row.checked = false;
                        self.rows.set_row_data(index, row);
                    } else {
                        self.rows.insert(index + 1, new_row("divider", ""));
                    }
                    let next = index
                        + if self
                            .rows
                            .row_data(index)
                            .is_some_and(|row| row.kind == "divider")
                        {
                            1
                        } else {
                            2
                        };
                    if next == self.rows.row_count() {
                        self.rows.push(new_row("text", ""));
                    }
                    self.focus(window, next, false);
                } else {
                    row.kind = value.into();
                    row.checked = false;
                    self.rows.set_row_data(index, row);
                    self.focus(window, index, true);
                }
            }
            BlockAction::Duplicate => {
                row.id = block_id();
                self.rows.insert(index + 1, row);
                self.focus(window, index + 1, true);
            }
            BlockAction::MoveUp | BlockAction::MoveDown => {
                let target = if command == BlockAction::MoveUp {
                    index.checked_sub(1)
                } else {
                    Some(index + 1).filter(|next| *next < count)
                };
                let Some(target) = target else { return false };
                self.rows.remove(index);
                self.rows.insert(target, row);
                self.focus(window, target, true);
            }
            BlockAction::Delete => {
                self.rows.remove(index);
                if count == 1 {
                    self.rows.push(new_row("text", ""));
                }
                self.focus(window, index.saturating_sub(1), true);
            }
            BlockAction::Toggle => {
                row.checked = !row.checked;
                self.rows.set_row_data(index, row);
            }
        }
        self.number_lists();
        true
    }

    fn focus(&self, window: &MainWindow, index: usize, at_end: bool) {
        if let Some(row) = self.rows.row_data(index) {
            window.set_focus_block(row.id);
            window.set_focus_offset(if at_end { row.text.len() as i32 } else { 0 });
            window.set_focus_request(window.get_focus_request().wrapping_add(1));
        }
    }

    fn number_lists(&self) {
        let mut number = 0;
        for (index, mut row) in self.rows.iter().enumerate() {
            number = if row.kind == "number" { number + 1 } else { 0 };
            if row.number != number {
                row.number = number;
                self.rows.set_row_data(index, row);
            }
        }
    }
}

fn split_selection(text: &str, anchor: i32, cursor: i32) -> (&str, &str) {
    let start = anchor.min(cursor).max(0) as usize;
    let end = anchor.max(cursor).max(0) as usize;
    match (text.get(..start), text.get(end..)) {
        (Some(head), Some(tail)) => (head, tail),
        _ => (text, ""),
    }
}

fn block_id() -> SharedString {
    Uuid::new_v4().simple().to_string().into()
}

fn new_row(kind: &str, text: &str) -> BlockRowData {
    BlockRowData {
        id: block_id(),
        kind: kind.into(),
        text: text.into(),
        checked: false,
        number: 0,
    }
}

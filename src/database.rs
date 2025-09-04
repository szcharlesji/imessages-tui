use chrono::{Datelike, Local, TimeZone};
use color_eyre::Result;
use dirs;
use rusqlite::Connection;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Chat {
    pub rowid: i64,
    pub guid: String,
    pub chat_identifier: String,
    pub display_name: Option<String>,
    pub service_name: String,
    pub is_group: bool,
    pub last_message_date: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub rowid: i64,
    pub text: Option<String>,
    pub attributed_body: Option<Vec<u8>>,
    pub is_from_me: bool,
    pub date: i64,
    pub handle_id: Option<i64>,
    pub service: String,
}

#[derive(Debug, Clone)]
pub struct Handle {
    pub rowid: i64,
    pub id: String,
    pub service: String,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(db_path: Option<PathBuf>) -> Result<Self> {
        let path = db_path.unwrap_or_else(|| {
            let mut home = dirs::home_dir().expect("Could not find home directory");
            home.push("Library/Messages/chat.db");
            home
        });

        let conn = Connection::open(&path)?;
        Ok(Database { conn })
    }

    pub fn get_chats(
        &self,
        known_only: bool,
        no_groups: bool,
        limit: Option<usize>,
    ) -> Result<Vec<Chat>> {
        let mut query = String::from(
            "SELECT 
                c.ROWID,
                c.guid,
                c.chat_identifier,
                c.display_name,
                c.service_name,
                CASE WHEN c.style = 45 THEN 1 ELSE 0 END as is_group,
                MAX(m.date) as last_message_date
            FROM chat c
            JOIN chat_message_join cmj ON c.ROWID = cmj.chat_id
            JOIN message m ON cmj.message_id = m.ROWID
            WHERE c.chat_identifier IS NOT NULL 
              AND c.chat_identifier != ''
              AND m.service = 'iMessage'",
        );

        if known_only {
            query.push_str(
                " AND c.display_name IS NOT NULL AND EXISTS (
                SELECT 1 FROM message m2 
                JOIN chat_message_join cmj2 ON m2.ROWID = cmj2.message_id 
                WHERE cmj2.chat_id = c.ROWID AND m2.is_from_me = 1
            )",
            );
        }

        if no_groups {
            query.push_str(" AND c.style != 45");
        }

        query.push_str(" GROUP BY c.ROWID ORDER BY last_message_date DESC");

        if let Some(limit) = limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }

        let mut stmt = self.conn.prepare(&query)?;
        let chat_iter = stmt.query_map([], |row| {
            Ok(Chat {
                rowid: row.get(0)?,
                guid: row.get(1)?,
                chat_identifier: row.get(2)?,
                display_name: row.get(3)?,
                service_name: row.get(4)?,
                is_group: row.get::<_, i32>(5)? == 1,
                last_message_date: row.get(6)?,
            })
        })?;

        let mut chats = Vec::new();
        for chat in chat_iter {
            chats.push(chat?);
        }

        Ok(chats)
    }

    pub fn get_messages(&self, chat_rowid: i64, limit: Option<usize>) -> Result<Vec<Message>> {
        let mut query = String::from(
            "SELECT 
                m.ROWID,
                m.text,
                m.attributedBody,
                m.is_from_me,
                m.date,
                m.handle_id,
                m.service
            FROM message m
            JOIN chat_message_join cmj ON m.ROWID = cmj.message_id
            WHERE cmj.chat_id = ?
              AND (m.text IS NOT NULL OR m.attributedBody IS NOT NULL)
            ORDER BY m.date ASC",
        );

        if let Some(limit) = limit {
            query = format!(
                "SELECT * FROM ({}) ORDER BY date DESC LIMIT {}",
                query.replace("ORDER BY m.date ASC", "ORDER BY m.date DESC"),
                limit
            );
        }

        let mut stmt = self.conn.prepare(&query)?;
        let message_iter = stmt.query_map([chat_rowid], |row| {
            Ok(Message {
                rowid: row.get(0)?,
                text: row.get(1)?,
                attributed_body: row.get(2)?,
                is_from_me: row.get::<_, i32>(3)? == 1,
                date: row.get(4)?,
                handle_id: row.get(5)?,
                service: row.get(6)?,
            })
        })?;

        let mut messages = Vec::new();
        for message in message_iter {
            messages.push(message?);
        }

        if limit.is_some() {
            messages.reverse();
        }

        Ok(messages)
    }

    pub fn send_message(&self, chat_identifier: &str, text: &str) -> Result<()> {
        let escaped_text = text.replace("\\", "\\\\").replace("\"", "\\\"");
        let script = format!(
            r#"tell application "Messages"
                set targetBuddy to buddy "{}" of (service 1 whose service type is iMessage)
                send "{}" to targetBuddy
            end tell"#,
            chat_identifier, escaped_text
        );

        let output = Command::new("osascript").arg("-e").arg(&script).output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(color_eyre::eyre::eyre!("AppleScript error: {}", error));
        }

        Ok(())
    }
}

/// Extract text content from NSAttributedString BLOB data
pub fn extract_text_from_attributed_body(attributed_body: &[u8]) -> Option<String> {
    if attributed_body.is_empty() {
        return None;
    }

    // Look for NSString pattern
    let ns_string_marker = b"NSString";
    if let Some(ns_string_pos) = attributed_body
        .windows(ns_string_marker.len())
        .position(|window| window == ns_string_marker)
    {
        let start_pos = ns_string_pos + ns_string_marker.len();
        if start_pos + 5 >= attributed_body.len() {
            return None;
        }

        // Skip some preamble bytes (typically 5 bytes after NSString)
        let text_part = &attributed_body[start_pos + 5..];
        
        if text_part.is_empty() {
            return None;
        }

        // Parse length encoding
        let (text_length, text_start) = if text_part[0] == 0x81 {
            // Extended length encoding (2-byte little-endian)
            if text_part.len() < 3 {
                return None;
            }
            let length = u16::from_le_bytes([text_part[1], text_part[2]]) as usize;
            (length, 3)
        } else {
            // Single byte length
            let length = text_part[0] as usize;
            (length, 1)
        };

        // Extract the text
        if text_start + text_length <= text_part.len() {
            let text_bytes = &text_part[text_start..text_start + text_length];
            
            // Try to decode as UTF-8
            match String::from_utf8(text_bytes.to_vec()) {
                Ok(text) => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
                Err(_) => {
                    // Try with lossy conversion
                    let text = String::from_utf8_lossy(text_bytes);
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    // Fallback: try to find printable ASCII/UTF-8 text in the blob
    match String::from_utf8_lossy(attributed_body).trim() {
        text if text.len() > 2 && text.chars().any(|c| c.is_alphabetic() || c.is_numeric()) => {
            let cleaned: String = text.chars()
                .filter(|c| c.is_ascii_graphic() || c.is_whitespace() || *c as u32 > 127)
                .collect();
            if cleaned.trim().len() > 2 {
                Some(cleaned.trim().to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Get the actual message text from text field or attributedBody
pub fn get_message_text(text_field: Option<&String>, attributed_body: Option<&Vec<u8>>) -> String {
    // First try the regular text field
    if let Some(text) = text_field {
        let trimmed = text.trim();
        if !trimmed.is_empty() && trimmed != "￼" {
            return trimmed.to_string();
        }
    }

    // Try to extract from attributedBody
    if let Some(blob) = attributed_body {
        if let Some(extracted) = extract_text_from_attributed_body(blob) {
            let trimmed = extracted.trim();
            if !trimmed.is_empty() && trimmed != "￼" {
                return trimmed.to_string();
            }
        }
    }

    // Handle attachment indicator
    if let Some(text) = text_field {
        if text.trim() == "￼" {
            return "[Attachment]".to_string();
        }
    }

    "[No readable text]".to_string()
}


pub fn format_timestamp(timestamp: i64) -> String {
    // Convert from Mac epoch (2001-01-01) to Unix epoch (1970-01-01)
    let unix_timestamp = timestamp / 1_000_000_000 + 978_307_200;

    if let Some(datetime) = Local.timestamp_opt(unix_timestamp, 0).single() {
        let now = Local::now();
        if datetime.date_naive() == now.date_naive() {
            datetime.format("%H:%M").to_string()
        } else if datetime.date_naive().year() == now.date_naive().year() {
            datetime.format("%m/%d %H:%M").to_string()
        } else {
            datetime.format("%Y/%m/%d %H:%M").to_string()
        }
    } else {
        String::new()
    }
}


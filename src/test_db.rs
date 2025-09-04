use crate::database::Database;
use color_eyre::Result;

pub fn test_database_connection() -> Result<()> {
    println!("Testing database connection...");

    let db = Database::new(None)?;
    println!("✓ Database connection successful");

    let chats = db.get_chats(false, false, Some(5))?;
    println!("✓ Found {} chats", chats.len());

    for (i, chat) in chats.iter().take(3).enumerate() {
        let name = chat
            .display_name
            .as_deref()
            .unwrap_or(&chat.chat_identifier);
        let group_indicator = if chat.is_group { " (Group)" } else { "" };
        println!("  {}. {}{}", i + 1, name, group_indicator);
    }

    if !chats.is_empty() {
        let messages = db.get_messages(chats[0].rowid, Some(3))?;
        println!("✓ Found {} messages in first chat", messages.len());

        for message in messages.iter().take(2) {
            let text = message.text.as_deref().unwrap_or("<no text>");
            let sender = if message.is_from_me { "Me" } else { "Other" };
            println!(
                "  {}: {} (rowid: {}, len: {})",
                sender,
                text.chars().take(50).collect::<String>(),
                message.rowid,
                text.len()
            );
        }
    }

    Ok(())
}


use crate::contacts::ContactsManager;
use crate::database::{Database, get_message_text};
use color_eyre::Result;

pub fn test_database_connection() -> Result<()> {
    println!("Testing database connection with contacts...");

    // Test database connection
    let db = Database::new(None)?;
    println!("✓ Database connection successful");

    // Load contacts
    let mut contacts = ContactsManager::new();
    contacts.load_contacts()?;

    let chats = db.get_chats(false, false, Some(5))?;
    println!("✓ Found {} chats", chats.len());

    for (i, chat) in chats.iter().take(3).enumerate() {
        let name = contacts.get_display_name(&chat.chat_identifier, chat.display_name.as_deref());
        let known_indicator = if contacts.is_known_contact(&chat.chat_identifier) { "👤" } else { "❓" };
        let group_indicator = if chat.is_group { " (Group)" } else { "" };
        println!("  {}. {} {}{}", i + 1, known_indicator, name, group_indicator);
    }

    if !chats.is_empty() {
        let messages = db.get_messages(chats[0].rowid, Some(3))?;
        println!("✓ Found {} messages in first chat", messages.len());

        for message in messages.iter().take(2) {
            let text = get_message_text(message.text.as_ref(), message.attributed_body.as_ref());
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

    println!("✓ Total contacts loaded: {}", contacts.contact_count());
    Ok(())
}


use color_eyre::Result;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ContactsManager {
    contacts_cache: HashMap<String, String>,
}

impl ContactsManager {
    pub fn new() -> Self {
        Self {
            contacts_cache: HashMap::new(),
        }
    }

    /// Load all contacts from macOS Contacts app using AppleScript
    pub fn load_contacts(&mut self) -> Result<()> {
        println!("📖 Loading contacts from Contacts app...");

        let script = r#"
        tell application "Contacts"
            set contactList to {}
            repeat with aPerson in people
                set contactName to name of aPerson
                set phoneList to {}
                set emailList to {}
                
                -- Get all phone numbers
                repeat with aPhone in phones of aPerson
                    set end of phoneList to (value of aPhone as string)
                end repeat
                
                -- Get all email addresses  
                repeat with anEmail in emails of aPerson
                    set end of emailList to (value of anEmail as string)
                end repeat
                
                -- Format: "name:|phone1,phone2|email1,email2"
                set phoneStr to ""
                repeat with i from 1 to count of phoneList
                    if i > 1 then set phoneStr to phoneStr & ","
                    set phoneStr to phoneStr & (item i of phoneList)
                end repeat
                
                set emailStr to ""
                repeat with i from 1 to count of emailList
                    if i > 1 then set emailStr to emailStr & ","
                    set emailStr to emailStr & (item i of emailList)
                end repeat
                
                set contactEntry to contactName & "|" & phoneStr & "|" & emailStr
                set end of contactList to contactEntry
            end repeat
            
            return contactList
        end tell
        "#;

        match Command::new("osascript").arg("-e").arg(script).output() {
            Ok(output) => {
                if output.status.success() {
                    let contacts_data = String::from_utf8_lossy(&output.stdout);
                    self.parse_contacts_result(&contacts_data)?;
                } else {
                    let error = String::from_utf8_lossy(&output.stderr);
                    eprintln!("❌ AppleScript error: {}", error);
                }
            }
            Err(e) => {
                eprintln!("❌ Error executing AppleScript: {}", e);
            }
        }

        Ok(())
    }

    /// Parse the AppleScript result into a lookup dictionary
    fn parse_contacts_result(&mut self, contacts_string: &str) -> Result<()> {
        if contacts_string.is_empty() || contacts_string.trim() == "missing value" {
            return Ok(());
        }

        // Remove surrounding braces and split by ", " but be careful with internal commas
        let cleaned = contacts_string
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}');

        // Split contacts - need to handle the complex parsing
        let entries = self.split_contact_entries(cleaned);

        for entry in entries {
            if !entry.contains('|') {
                continue;
            }

            let parts: Vec<&str> = entry.split('|').collect();
            if parts.len() >= 3 {
                let name = parts[0].trim();
                let phones: Vec<&str> = parts[1]
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();
                let emails: Vec<&str> = parts[2]
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect();

                // Create lookup entries for all phone numbers and emails
                for phone in phones {
                    if let Some(normalized_phone) = self.normalize_phone(phone) {
                        self.contacts_cache
                            .insert(normalized_phone, name.to_string());
                        self.contacts_cache
                            .insert(phone.to_string(), name.to_string());
                    }
                }

                for email in emails {
                    if !email.is_empty() {
                        self.contacts_cache
                            .insert(email.to_lowercase(), name.to_string());
                    }
                }
            }
        }

        Ok(())
    }

    /// Split contact entries, handling commas within entries
    fn split_contact_entries(&self, contacts_string: &str) -> Vec<String> {
        let mut entries = Vec::new();
        let mut current_entry = String::new();
        let mut pipe_count = 0;
        let chars: Vec<char> = contacts_string.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            let ch = chars[i];

            if ch == '|' {
                pipe_count += 1;
                current_entry.push(ch);
            } else if ch == ',' && pipe_count >= 2 {
                // This might be a separator between entries
                if i + 1 < chars.len() && chars[i + 1] == ' ' {
                    // Look ahead to see if this looks like a new entry
                    let remaining = &chars[i + 2..];
                    let next_part: String = remaining.iter().take(50).collect();
                    if next_part.contains('|') {
                        // This is likely an entry separator
                        entries.push(current_entry.trim().to_string());
                        current_entry.clear();
                        pipe_count = 0;
                        i += 2; // Skip ", "
                        continue;
                    }
                }
                current_entry.push(ch);
            } else {
                current_entry.push(ch);
            }

            i += 1;
        }

        if !current_entry.trim().is_empty() {
            entries.push(current_entry.trim().to_string());
        }

        entries
    }

    /// Normalize phone number for consistent lookup
    fn normalize_phone(&self, phone: &str) -> Option<String> {
        if phone.is_empty() {
            return None;
        }

        // Remove all non-digits
        let digits_only: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();

        if digits_only.is_empty() {
            return None;
        }

        // Handle US numbers
        match digits_only.len() {
            10 => Some(format!("+1{}", digits_only)),
            11 if digits_only.starts_with('1') => Some(format!("+{}", digits_only)),
            _ if digits_only.len() > 11 && digits_only.starts_with('1') => {
                Some(format!("+{}", digits_only))
            }
            _ => Some(format!("+{}", digits_only)),
        }
    }

    /// Get contact name for phone number or email
    pub fn get_contact_name(&self, identifier: &str) -> Option<&String> {
        if identifier.is_empty() {
            return None;
        }

        // Direct lookup
        if let Some(name) = self.contacts_cache.get(identifier) {
            return Some(name);
        }

        // Try lowercase for emails
        if identifier.contains('@') {
            return self.contacts_cache.get(&identifier.to_lowercase());
        }

        // Try normalized phone lookup
        if identifier.starts_with('+') || identifier.chars().any(|c| c.is_ascii_digit()) {
            if let Some(normalized) = self.normalize_phone(identifier) {
                if let Some(name) = self.contacts_cache.get(&normalized) {
                    return Some(name);
                }

                // Try without country code
                if normalized.starts_with("+1") {
                    let local_number = &normalized[2..];
                    for (cached_number, cached_name) in &self.contacts_cache {
                        if cached_number.ends_with(local_number) {
                            return Some(cached_name);
                        }
                    }
                }
            }
        }

        None
    }

    /// Get a friendly display name, using contact name or provided fallback
    pub fn get_display_name(&self, identifier: &str, fallback: Option<&str>) -> String {
        if let Some(contact_name) = self.get_contact_name(identifier) {
            contact_name.clone()
        } else if let Some(fallback_name) = fallback {
            fallback_name.to_string()
        } else {
            self.format_identifier(identifier)
        }
    }

    /// Format an identifier (phone/email) for display when no contact name is found
    fn format_identifier(&self, identifier: &str) -> String {
        // Handle phone numbers
        if identifier.starts_with('+') && identifier[1..].chars().all(|c| c.is_ascii_digit()) {
            return identifier.to_string();
        }

        // Handle email addresses - show first part if too long
        if identifier.contains('@') {
            if identifier.len() > 25 {
                if let Some(at_pos) = identifier.find('@') {
                    let local_part = &identifier[..at_pos];
                    let domain_part = &identifier[at_pos..];
                    if local_part.len() > 15 {
                        return format!("{}...{}", &local_part[..12], domain_part);
                    }
                }
            }
            return identifier.to_string();
        }

        // For other identifiers, truncate if too long
        if identifier.len() > 25 {
            format!("{}...", &identifier[..22])
        } else {
            identifier.to_string()
        }
    }

    /// Get contact count for debugging
    pub fn contact_count(&self) -> usize {
        self.contacts_cache.len()
    }

    /// Check if a contact is known (has a name in contacts)
    pub fn is_known_contact(&self, identifier: &str) -> bool {
        self.get_contact_name(identifier).is_some()
    }
}


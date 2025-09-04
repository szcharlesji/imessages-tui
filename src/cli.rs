use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Custom path to the Messages database
    #[arg(long, value_name = "PATH")]
    pub db_path: Option<PathBuf>,

    /// Maximum number of messages to load per chat
    #[arg(long, default_value_t = 100)]
    pub limit: usize,

    /// Only show chats with display names where you have sent messages
    #[arg(long)]
    pub known_only: bool,

    /// Filter out group chats
    #[arg(long)]
    pub no_groups: bool,

    /// Maximum number of chats to load
    #[arg(long, default_value_t = 50)]
    pub chat_limit: usize,
}

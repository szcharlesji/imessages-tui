mod cli;
mod contacts;
mod database;
mod test_db;

use clap::Parser;
use cli::Args;
use color_eyre::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use contacts::ContactsManager;
use database::{Chat, Database, Message};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use std::io;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    // Test database connection first (uncomment for debugging)
    // test_db::test_database_connection()?;

    // Check if terminal setup will work
    println!("Starting iMessages TUI...");
    println!("Use j/k to navigate chats, Ctrl+D/P to scroll messages, Enter to send, q to quit");

    // Setup terminal
    enable_raw_mode().map_err(|e| {
        eprintln!("Failed to enable raw mode: {}", e);
        eprintln!("This typically means the application isn't running in a proper terminal.");
        eprintln!("Please run this in Terminal.app, iTerm2, or another terminal emulator.");
        e
    })?;
    
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).map_err(|e| {
        disable_raw_mode().ok();
        eprintln!("Failed to setup terminal screen: {}", e);
        e
    })?;
    
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend).map_err(|e| {
        disable_raw_mode().ok();
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture).ok();
        eprintln!("Failed to create terminal: {}", e);
        e
    })?;

    // Run app
    let result = App::new(args)?.run(terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    result
}

/// The main application which holds the state and logic of the application.
pub struct App {
    running: bool,
    database: Database,
    contacts: ContactsManager,
    chats: Vec<Chat>,
    messages: Vec<Message>,
    chat_list_state: ListState,
    message_scroll: usize,
    input: String,
    input_mode: bool,
    args: Args,
}

impl App {
    /// Construct a new instance of [`App`].
    pub fn new(args: Args) -> Result<Self> {
        let database = Database::new(args.db_path.clone())?;
        
        // Load contacts from Contacts app
        let mut contacts = ContactsManager::new();
        contacts.load_contacts()?;
        
        let chats = database.get_chats(args.known_only, args.no_groups, Some(args.chat_limit))?;

        let mut chat_list_state = ListState::default();
        if !chats.is_empty() {
            chat_list_state.select(Some(0));
        }

        let messages = if !chats.is_empty() {
            database.get_messages(chats[0].rowid, Some(args.limit))?
        } else {
            Vec::new()
        };

        Ok(Self {
            running: false,
            database,
            contacts,
            chats,
            messages,
            chat_list_state,
            message_scroll: 0,
            input: String::new(),
            input_mode: false,
            args,
        })
    }

    /// Run the application's main loop.
    pub fn run<B: Backend>(mut self, mut terminal: Terminal<B>) -> Result<()> {
        self.running = true;
        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_crossterm_events()?;
        }
        Ok(())
    }

    /// Renders the user interface.
    fn render(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(frame.area());

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(4)])
            .split(chunks[1]);

        // Chat list (left panel)
        self.render_chat_list(frame, chunks[0]);

        // Message view (right top panel)
        self.render_messages(frame, right_chunks[0]);

        // Input box and instructions (right bottom panel)
        self.render_input(frame, right_chunks[1]);
    }

    fn render_chat_list(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .chats
            .iter()
            .map(|chat| {
                let name = self.contacts.get_display_name(&chat.chat_identifier);
                let group_indicator = if chat.is_group { " (Group)" } else { "" };
                ListItem::new(format!("{}{}", name, group_indicator))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Chats"))
            .highlight_style(Style::default().bg(Color::Blue).fg(Color::White))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, area, &mut self.chat_list_state);
    }

    fn render_messages(&mut self, frame: &mut Frame, area: Rect) {
        let chat_name = if let Some(selected) = self.chat_list_state.selected() {
            self.chats
                .get(selected)
                .map(|chat| self.contacts.get_display_name(&chat.chat_identifier))
                .unwrap_or_else(|| "No Chat Selected".to_string())
        } else {
            "No Chat Selected".to_string()
        };

        let message_lines: Vec<Line> = self
            .messages
            .iter()
            .skip(self.message_scroll)
            .map(|msg| {
                let text = database::get_message_text(msg.text.as_ref(), msg.attributed_body.as_ref());
                let timestamp = database::format_timestamp(msg.date);
                let sender = if msg.is_from_me { "Me" } else { &chat_name };

                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", timestamp),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(
                        format!("{}: ", sender),
                        Style::default().fg(if msg.is_from_me {
                            Color::Blue
                        } else {
                            Color::Green
                        }),
                    ),
                    Span::raw(text),
                ])
            })
            .collect();

        let messages_widget = Paragraph::new(message_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Messages - {}", chat_name)),
            )
            .wrap(Wrap { trim: true });

        frame.render_widget(messages_widget, area);
    }

    fn render_input(&mut self, frame: &mut Frame, area: Rect) {
        let input_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(3)])
            .split(area);

        // Instructions based on current mode
        let instructions_text = if self.input_mode {
            "TYPING MODE: Type your message | Enter: send | Esc: exit typing mode | Ctrl+C: quit"
        } else {
            "NAV MODE: j/k: nav chats | Ctrl+D/P: scroll msgs | Enter: start typing | q/Esc: quit"
        };
        
        let instructions = Paragraph::new(instructions_text)
            .style(Style::default().fg(Color::Gray));
        frame.render_widget(instructions, input_chunks[0]);

        // Input box with highlighting based on mode
        let input_block = if self.input_mode {
            Block::default()
                .borders(Borders::ALL)
                .title("[TYPING] Type message")
                .border_style(Style::default().fg(Color::Green))
        } else {
            Block::default()
                .borders(Borders::ALL)
                .title("Press Enter to type")
                .border_style(Style::default().fg(Color::Gray))
        };
        
        let input = Paragraph::new(self.input.as_str()).block(input_block);
        frame.render_widget(input, input_chunks[1]);

        // Show cursor (limit position to avoid overflow)
        let cursor_x = (input_chunks[1].x + self.input.len() as u16 + 1)
            .min(input_chunks[1].x + input_chunks[1].width - 2);
        frame.set_cursor_position((cursor_x, input_chunks[1].y + 1));
    }

    /// Reads the crossterm events and updates the state of [`App`].
    fn handle_crossterm_events(&mut self) -> Result<()> {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            _ => {}
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    fn on_key_event(&mut self, key: KeyEvent) {
        if self.input_mode {
            // Input mode: handle typing and sending
            match (key.modifiers, key.code) {
                // Global quit (Ctrl+C) - check first before char matching
                (KeyModifiers::CONTROL, KeyCode::Char('c')) => self.quit(),
                // Exit input mode
                (_, KeyCode::Esc) => {
                    self.input_mode = false;
                }
                // Send message
                (_, KeyCode::Enter) => {
                    self.send_message();
                }
                // Input handling
                (_, KeyCode::Backspace) => {
                    self.input.pop();
                }
                // Type characters (including j, k, q)
                (_, KeyCode::Char(c)) => {
                    self.input.push(c);
                }
                _ => {}
            }
        } else {
            // Navigation mode: handle navigation and mode switching
            match (key.modifiers, key.code) {
                // Quit
                (_, KeyCode::Esc | KeyCode::Char('q'))
                | (KeyModifiers::CONTROL, KeyCode::Char('c')) => self.quit(),

                // Chat navigation
                (_, KeyCode::Char('j') | KeyCode::Down) => self.select_next_chat(),
                (_, KeyCode::Char('k') | KeyCode::Up) => self.select_previous_chat(),

                // Message scrolling
                (KeyModifiers::CONTROL, KeyCode::Char('d')) => self.scroll_messages_down(),
                (KeyModifiers::CONTROL, KeyCode::Char('p')) => self.scroll_messages_up(),

                // Enter input mode
                (_, KeyCode::Enter) => {
                    self.input_mode = true;
                }

                _ => {}
            }
        }
    }

    fn select_next_chat(&mut self) {
        if self.chats.is_empty() {
            return;
        }

        let selected = self.chat_list_state.selected().unwrap_or(0);
        let next = (selected + 1) % self.chats.len();
        self.chat_list_state.select(Some(next));
        self.load_messages_for_selected_chat();
    }

    fn select_previous_chat(&mut self) {
        if self.chats.is_empty() {
            return;
        }

        let selected = self.chat_list_state.selected().unwrap_or(0);
        let prev = if selected == 0 {
            self.chats.len() - 1
        } else {
            selected - 1
        };
        self.chat_list_state.select(Some(prev));
        self.load_messages_for_selected_chat();
    }

    fn scroll_messages_down(&mut self) {
        if self.message_scroll < self.messages.len().saturating_sub(10) {
            self.message_scroll += 5;
        }
    }

    fn scroll_messages_up(&mut self) {
        self.message_scroll = self.message_scroll.saturating_sub(5);
    }

    fn send_message(&mut self) {
        if self.input.trim().is_empty() {
            return;
        }

        if let Some(selected) = self.chat_list_state.selected() {
            if let Some(chat) = self.chats.get(selected) {
                if let Err(e) = self
                    .database
                    .send_message(&chat.chat_identifier, &self.input)
                {
                    eprintln!("Failed to send message: {}", e);
                } else {
                    self.input.clear();
                    self.input_mode = false; // Exit input mode after sending
                    // Reload messages after sending
                    self.load_messages_for_selected_chat();
                }
            }
        }
    }

    fn load_messages_for_selected_chat(&mut self) {
        if let Some(selected) = self.chat_list_state.selected() {
            if let Some(chat) = self.chats.get(selected) {
                if let Ok(messages) = self
                    .database
                    .get_messages(chat.rowid, Some(self.args.limit))
                {
                    self.messages = messages;
                    self.message_scroll = 0;
                }
            }
        }
    }

    /// Set running to false to quit the application.
    fn quit(&mut self) {
        self.running = false;
    }
}

//! oxt-tui — Interfaz TUI con ratatui
//!
//! Explorador de archivos + vista previa de documentos.
//! Panel izquierdo: navegación de directorios.
//! Panel derecho: contenido del documento seleccionado.

use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, List, ListItem};
use ratatui::Terminal;
use std::io;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = app.run(&mut terminal);

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;

    if let Err(e) = res {
        eprintln!("Error: {e}");
    }

    Ok(())
}

struct App {
    current_dir: std::path::PathBuf,
    entries: Vec<std::path::PathBuf>,
    selected: usize,
    preview: Option<String>,
    quit: bool,
}

impl App {
    fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut app = Self {
            entries: Vec::new(),
            selected: 0,
            preview: None,
            quit: false,
            current_dir,
        };
        app.refresh_dir();
        app
    }

    fn refresh_dir(&mut self) {
        let mut entries = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&self.current_dir) {
            for entry in read_dir.flatten() {
                entries.push(entry.path());
            }
        }
        entries.sort();
        self.entries = entries;
        self.selected = 0;
        self.preview = None;
    }

    fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
        while !self.quit {
            terminal.draw(|f| self.render(f))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn render(&self, frame: &mut ratatui::Frame) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(frame.area());

        // Panel izquierdo: explorador
        let items: Vec<ListItem> = self.entries.iter().map(|p| {
            let name = p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_dir = p.is_dir();
            let prefix = if is_dir { "📁 " } else { "📄 " };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}", prefix, name),
                Style::default(),
            )))
        }).collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Archivos"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, layout[0], &mut ratatui::widgets::ListState::default().with_selected(Some(self.selected)));

        // Panel derecho: preview
        let preview_text = match self.preview {
            Some(ref text) => text.clone(),
            None => "Selecciona un archivo".to_string(),
        };

        let preview = Paragraph::new(preview_text)
            .block(Block::default().borders(Borders::ALL).title("Vista previa"))
            .style(Style::default());

        frame.render_widget(preview, layout[1]);
    }

    fn handle_events(&mut self) -> io::Result<()> {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => self.quit = true,
                    KeyCode::Up => {
                        if self.selected > 0 {
                            self.selected -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if self.selected + 1 < self.entries.len() {
                            self.selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(path) = self.entries.get(self.selected).cloned() {
                            if path.is_dir() {
                                self.current_dir = path;
                                self.refresh_dir();
                            } else {
                                self.load_preview(&path);
                            }
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(parent) = self.current_dir.parent() {
                            self.current_dir = parent.to_path_buf();
                            self.refresh_dir();
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn load_preview(&mut self, path: &std::path::Path) {
        let preview = match oxt_backend::Document::open(path) {
            Ok(doc) => {
                let ir = doc.to_ir();
                ir.to_markdown()
            }
            Err(_) => {
                // Si no es documento office, mostrar como texto plano
                std::fs::read_to_string(path).unwrap_or_else(|_| {
                    format!("(No se puede previsualizar: {})", path.display())
                })
            }
        };
        self.preview = Some(preview);
    }
}

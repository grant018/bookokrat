use crate::theme::current_theme;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// `j`, `k` and `q` drive the list, so a target starting with one of them
/// simply gets no hotkey rather than shadowing navigation.
const RESERVED: [char; 3] = ['j', 'k', 'q'];

pub enum LookupPickerAction {
    Run(String),
    Cancel,
    Redraw,
    Ignored,
}

pub struct LookupPicker {
    targets: Vec<String>,
    hotkeys: Vec<Option<char>>,
    selected: usize,
    query: String,
}

impl LookupPicker {
    pub fn new(targets: Vec<String>, query: String) -> Self {
        let mut taken: Vec<char> = Vec::new();
        let hotkeys = targets
            .iter()
            .map(|t| match t.chars().next().map(|c| c.to_ascii_lowercase()) {
                Some(c) if !RESERVED.contains(&c) && !taken.contains(&c) => {
                    taken.push(c);
                    Some(c)
                }
                _ => None,
            })
            .collect();
        Self {
            targets,
            hotkeys,
            selected: 0,
            query,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    fn step(&mut self, delta: isize) {
        let len = self.targets.len() as isize;
        if len == 0 {
            return;
        }
        self.selected = (((self.selected as isize + delta) % len + len) % len) as usize;
    }

    pub fn handle_key(&mut self, code: &KeyCode) -> LookupPickerAction {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => LookupPickerAction::Cancel,
            KeyCode::Down | KeyCode::Char('j') => {
                self.step(1);
                LookupPickerAction::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.step(-1);
                LookupPickerAction::Redraw
            }
            KeyCode::Enter => match self.targets.get(self.selected) {
                Some(t) => LookupPickerAction::Run(t.clone()),
                None => LookupPickerAction::Cancel,
            },
            KeyCode::Char(c) => {
                let c = c.to_ascii_lowercase();
                match self.hotkeys.iter().position(|h| *h == Some(c)) {
                    Some(i) => LookupPickerAction::Run(self.targets[i].clone()),
                    None => LookupPickerAction::Ignored,
                }
            }
            _ => LookupPickerAction::Ignored,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let theme = current_theme();
        let longest = self
            .targets
            .iter()
            .map(|t| t.chars().count())
            .max()
            .unwrap_or(0);

        let width = ((longest + 16) as u16)
            .clamp(34, 60)
            .min(area.width.saturating_sub(4).max(1));
        let height = (self.targets.len() as u16 + 4).min(area.height.saturating_sub(2).max(1));
        let popup = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        };
        f.render_widget(Clear, popup);

        let label_room = width.saturating_sub(4) as usize;
        let mut query = self.query.replace('\n', " ");
        if query.chars().count() > label_room.saturating_sub(10) {
            query = query
                .chars()
                .take(label_room.saturating_sub(13))
                .collect::<String>()
                + "...";
        }

        let block = Block::default()
            .title(Span::styled(
                format!(" Look up: {query} "),
                Style::default()
                    .fg(theme.base_0a)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.popup_border_color()))
            .style(Style::default().bg(theme.base_00).fg(theme.base_05));

        let mut lines: Vec<Line> = Vec::with_capacity(self.targets.len() + 1);
        for (i, name) in self.targets.iter().enumerate() {
            let selected = i == self.selected;
            let key = match self.hotkeys[i] {
                Some(c) => format!(" {c} "),
                None => "   ".to_string(),
            };
            let row = Style::default().bg(if selected {
                theme.base_02
            } else {
                theme.base_00
            });
            lines.push(Line::from(vec![
                Span::styled(if selected { " > " } else { "   " }, row.fg(theme.base_0d)),
                Span::styled(key, row.fg(theme.base_09).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{name:<label_room$}"),
                    if selected {
                        row.fg(theme.base_06).add_modifier(Modifier::BOLD)
                    } else {
                        row.fg(theme.base_05)
                    },
                ),
            ]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  j/k  enter run  esc cancel",
            Style::default().fg(theme.base_03),
        )));

        f.render_widget(Paragraph::new(lines).block(block), popup);
    }
}

use crate::inputs::KeySeq;
use crate::theme::current_theme;
use crate::widget::popup::Popup;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

pub enum LookupPopupAction {
    Close,
}

pub struct LookupPopup {
    title: String,
    lines: Vec<Line<'static>>,
    total_lines: usize,
    scroll_offset: usize,
    last_popup_area: Option<Rect>,
    pending: Option<PendingLookup>,
}

/// A lookup command still running in the background.
struct PendingLookup {
    rx: Receiver<Result<String, String>>,
    started: Instant,
    shown_secs: u64,
    #[cfg(unix)]
    pgid: i32,
}

impl LookupPopup {
    pub fn new(word: String, result: Result<String, String>) -> Self {
        let lines = result_lines(result);
        let total_lines = lines.len();

        Self {
            title: word,
            lines,
            total_lines,
            scroll_offset: 0,
            last_popup_area: None,
            pending: None,
        }
    }

    /// Run `command` in the background, showing a placeholder until its output
    /// arrives. Slow commands (an LLM, a network dictionary) would otherwise
    /// freeze the UI for as long as they take.
    pub fn spawn(word: String, mut command: Command) -> Self {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Its own process group, so closing the popup early stops the whole
        // pipeline rather than just the shell that launched it.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        let child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return Self::new(word, Err(format!("Failed to run command: {e}"))),
        };
        #[cfg(unix)]
        let pgid = child.id() as i32;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(command_result(child.wait_with_output()));
        });

        let mut popup = Self::new(word, Ok(String::new()));
        popup.set_lines(thinking_lines(0));
        popup.pending = Some(PendingLookup {
            rx,
            started: Instant::now(),
            shown_secs: 0,
            #[cfg(unix)]
            pgid,
        });
        popup
    }

    /// Pick up a finished background lookup and tick the elapsed-time counter.
    /// Returns true when the popup changed and needs a redraw.
    pub fn poll(&mut self) -> bool {
        let Some(pending) = self.pending.as_mut() else {
            return false;
        };
        let result = match pending.rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Disconnected) => Err("Lookup ended without a result".to_string()),
            Err(TryRecvError::Empty) => {
                let secs = pending.started.elapsed().as_secs();
                if secs == pending.shown_secs {
                    return false;
                }
                pending.shown_secs = secs;
                self.set_lines(thinking_lines(secs));
                return true;
            }
        };
        self.pending = None;
        self.set_lines(result_lines(result));
        true
    }

    /// Block until a background lookup finishes, for tests that snapshot the result.
    pub fn wait(&mut self) {
        if let Some(pending) = self.pending.take() {
            let result = pending
                .rx
                .recv()
                .unwrap_or_else(|_| Err("Lookup ended without a result".to_string()));
            self.set_lines(result_lines(result));
        }
    }

    fn set_lines(&mut self, lines: Vec<Line<'static>>) {
        self.total_lines = lines.len();
        self.lines = lines;
        self.scroll_offset = 0;
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        let popup_width = (area.width * 70 / 100)
            .max(40)
            .min(area.width.saturating_sub(4));
        let popup_height = (area.height * 70 / 100)
            .max(10)
            .min(area.height.saturating_sub(4));

        let popup_area = Rect {
            x: (area.width.saturating_sub(popup_width)) / 2,
            y: (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        self.last_popup_area = Some(popup_area);

        f.render_widget(Clear, popup_area);

        let visible_lines: Vec<Line> = self
            .lines
            .iter()
            .skip(self.scroll_offset)
            .cloned()
            .collect();

        let title = format!(" Lookup: {} ", self.title);
        let block = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(current_theme().base_0a)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(current_theme().popup_border_color()))
            .style(Style::default().bg(current_theme().base_00));

        let paragraph = Paragraph::new(visible_lines)
            .block(block)
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, popup_area);

        if self.total_lines > popup_height.saturating_sub(2) as usize {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(current_theme().base_04))
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"));

            let mut scrollbar_state =
                ScrollbarState::new(self.total_lines).position(self.scroll_offset);

            f.render_stateful_widget(
                scrollbar,
                popup_area.inner(ratatui::layout::Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                &mut scrollbar_state,
            );
        }
    }

    pub fn scroll_down(&mut self) {
        if self.scroll_offset < self.total_lines.saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn handle_key(
        &mut self,
        key: crossterm::event::KeyEvent,
        key_seq: &mut KeySeq,
    ) -> Option<LookupPopupAction> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_down();
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_up();
                None
            }
            KeyCode::Char('g') if key_seq.handle_key('g') == "gg" => {
                self.scroll_offset = 0;
                None
            }
            KeyCode::Char('G') => {
                self.scroll_offset = self.total_lines.saturating_sub(1);
                None
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let half = 10;
                self.scroll_offset =
                    (self.scroll_offset + half).min(self.total_lines.saturating_sub(1));
                None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let half = 10;
                self.scroll_offset = self.scroll_offset.saturating_sub(half);
                None
            }
            KeyCode::Esc | KeyCode::Char('q') => Some(LookupPopupAction::Close),
            _ => None,
        }
    }
}

impl Popup for LookupPopup {
    fn get_last_popup_area(&self) -> Option<Rect> {
        self.last_popup_area
    }
}

impl Drop for LookupPopup {
    /// Closing the popup before the answer arrives cancels the lookup.
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pending) = self.pending.take() {
            // A result already waiting means the process has been reaped and
            // its pgid may be reused, so only signal one still running.
            if matches!(pending.rx.try_recv(), Err(TryRecvError::Empty)) {
                unsafe {
                    libc::kill(-pending.pgid, libc::SIGTERM);
                }
            }
        }
    }
}

fn command_result(output: std::io::Result<Output>) -> Result<String, String> {
    match output {
        Ok(output) => {
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                if stderr.is_empty() {
                    Err(format!("Command exited with status {}", output.status))
                } else {
                    Err(stderr)
                }
            }
        }
        Err(e) => Err(format!("Failed to run command: {e}")),
    }
}

fn result_lines(result: Result<String, String>) -> Vec<Line<'static>> {
    match result {
        Ok(output) => {
            if output.trim().is_empty() {
                vec![Line::from(Span::styled(
                    "  (no output)",
                    Style::default().fg(current_theme().base_03),
                ))]
            } else if has_ansi_escapes(&output) {
                parse_ansi_output(&output)
            } else {
                output.lines().map(style_by_indent).collect()
            }
        }
        Err(err) => err
            .lines()
            .map(|l| {
                Line::from(Span::styled(
                    format!("  {l}"),
                    Style::default().fg(current_theme().base_08),
                ))
            })
            .collect(),
    }
}

fn thinking_lines(secs: u64) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        format!("  Thinking… {secs}s"),
        Style::default().fg(current_theme().base_03),
    ))]
}

/// Style a line based on its indentation level using theme colors.
/// No indent = heading/source (bold + accent), shallow = entry, deep = body text.
fn style_by_indent(line: &str) -> Line<'static> {
    let theme = current_theme();

    if line.trim().is_empty() {
        return Line::from(Span::raw(""));
    }

    let indent = line.len() - line.trim_start().len();

    let style = match indent {
        0 => Style::default()
            .fg(theme.base_0d)
            .add_modifier(Modifier::BOLD),
        1..=5 => Style::default()
            .fg(theme.base_07)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(theme.base_07),
    };

    Line::from(Span::styled(format!("  {line}"), style))
}

fn has_ansi_escapes(text: &str) -> bool {
    text.contains("\x1b[")
}

/// Parse output containing ANSI escape codes using vt100.
fn parse_ansi_output(text: &str) -> Vec<Line<'static>> {
    let line_count = text.lines().count().max(1);
    let max_width = text.lines().map(|l| l.len()).max().unwrap_or(80).max(80);

    let mut parser = vt100::Parser::new(line_count as u16, max_width as u16, 0);
    parser.process(text.as_bytes());

    let screen = parser.screen().clone();
    let theme = current_theme();
    let mut lines: Vec<Line<'static>> = Vec::new();

    for row in 0..line_count as u16 {
        let mut spans = Vec::new();
        spans.push(Span::raw("  "));

        let mut col = 0u16;
        while col < max_width as u16 {
            let Some(cell) = screen.cell(row, col) else {
                break;
            };

            let ch = if cell.contents().is_empty() {
                " ".to_string()
            } else {
                cell.contents()
            };

            let fg = vt100_color_to_ratatui(cell.fgcolor(), theme.base_07);
            let bg = vt100_color_to_ratatui(cell.bgcolor(), Color::Reset);

            let final_bg = if matches!(bg, Color::Reset) {
                theme.base_00
            } else {
                bg
            };

            let mut style = Style::default().fg(fg).bg(final_bg);

            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }

            spans.push(Span::styled(ch, style));
            col += 1;
        }

        // Trim trailing spaces
        while spans.last().is_some_and(|s| s.content.as_ref() == " ") {
            spans.pop();
        }

        lines.push(Line::from(spans));
    }

    // Trim trailing empty lines
    while lines
        .last()
        .is_some_and(|l| l.spans.iter().all(|s| s.content.as_ref().trim().is_empty()))
    {
        lines.pop();
    }

    lines
}

fn vt100_color_to_ratatui(c: vt100::Color, default: Color) -> Color {
    let theme = current_theme();
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => match i {
            0 => theme.base_00,
            1 => theme.base_08,
            2 => theme.base_0b,
            3 => theme.base_0a,
            4 => theme.base_0d,
            5 => theme.base_0e,
            6 => theme.base_0c,
            7 => theme.base_05,
            8 => theme.base_03,
            9 => theme.base_08,
            10 => theme.base_0b,
            11 => theme.base_0a,
            12 => theme.base_0d,
            13 => theme.base_0e,
            14 => theme.base_0c,
            15 => theme.base_07,
            _ => Color::Indexed(i),
        },
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

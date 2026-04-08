// onboarding_dialog.rs — First-launch welcome / onboarding dialog.
//
// Mirrors the TypeScript first-launch experience:
// - Shown once on first run (when Settings.has_completed_onboarding == false).
// - Walks the user through a brief orientation: key bindings, model info, help.
// - Dismissed by pressing Enter or Esc; sets has_completed_onboarding in settings.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use ratatui::Frame;

use crate::overlays::centered_rect;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Which page of the onboarding flow we're on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnboardingPage {
    #[default]
    Welcome,
    KeyBindings,
    Done,
}

/// State for the first-launch onboarding dialog.
#[derive(Debug, Default, Clone)]
pub struct OnboardingDialogState {
    /// Whether the dialog is currently visible.
    pub visible: bool,
    /// Current page.
    pub page: OnboardingPage,
}

impl OnboardingDialogState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the dialog (called at startup when first-run is detected).
    pub fn show(&mut self) {
        self.visible = true;
        self.page = OnboardingPage::Welcome;
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
    }

    /// Advance to the next page; returns true if we've reached Done and should dismiss.
    pub fn next_page(&mut self) -> bool {
        self.page = match self.page {
            OnboardingPage::Welcome => OnboardingPage::KeyBindings,
            OnboardingPage::KeyBindings => OnboardingPage::Done,
            OnboardingPage::Done => OnboardingPage::Done,
        };
        self.page == OnboardingPage::Done
    }

    /// Go back to the previous page.
    pub fn prev_page(&mut self) {
        self.page = match self.page {
            OnboardingPage::Welcome => OnboardingPage::Welcome,
            OnboardingPage::KeyBindings => OnboardingPage::Welcome,
            OnboardingPage::Done => OnboardingPage::KeyBindings,
        };
    }

    pub fn is_done(&self) -> bool {
        self.page == OnboardingPage::Done
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub fn render_onboarding_dialog(
    frame: &mut Frame,
    state: &OnboardingDialogState,
    area: Rect,
) {
    if !state.visible {
        return;
    }

    let dialog_width = 72u16.min(area.width.saturating_sub(4));
    let dialog_height = 26u16.min(area.height.saturating_sub(4));
    let dialog_area = centered_rect(dialog_width, dialog_height, area);

    frame.render_widget(Clear, dialog_area);

    match state.page {
        OnboardingPage::Welcome => render_welcome_page(frame, dialog_area),
        OnboardingPage::KeyBindings => render_keybindings_page(frame, dialog_area),
        OnboardingPage::Done => {} // should not be visible
    }
}

fn render_welcome_page(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![Span::styled(
            " Welcome to Claurst ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )]))
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line<'static>> = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Claurst is an AI-powered coding assistant in your terminal.",
            Style::default().fg(Color::White),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  How to use:",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            "    • Type your request and press Enter to send it.",
            Style::default().fg(Color::White),
        )]),
        Line::from(vec![Span::styled(
            "    • Claude can read, edit, and create files in your project.",
            Style::default().fg(Color::White),
        )]),
        Line::from(vec![Span::styled(
            "    • Claude can run bash commands, search the web, and more.",
            Style::default().fg(Color::White),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Slash commands:",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            "    /help       — show all commands",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(vec![Span::styled(
            "    /model      — switch AI model",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(vec![Span::styled(
            "    /compact    — summarise conversation to save context",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(vec![Span::styled(
            "    /cost       — show token usage and cost",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Page 1/2",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  → or Enter: next  ·  Esc: skip",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )]),
    ];

    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .render(inner, frame.buffer_mut());
}

fn render_keybindings_page(frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![Span::styled(
            " Keyboard Shortcuts ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )]))
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner into two columns
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let left_lines: Vec<Line<'static>> = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Input",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled("  Enter        send message", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  Shift+Enter  newline", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  Ctrl+C       interrupt/cancel", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  Ctrl+L       clear screen", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  ↑↓           history", Style::default().fg(Color::White))]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Navigation",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled("  PgUp/PgDn    scroll transcript", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  Ctrl+K       clear input", Style::default().fg(Color::White))]),
    ];

    let right_lines: Vec<Line<'static>> = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Permissions",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled("  y  allow tool once", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  Y  allow all this session", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  n  deny tool", Style::default().fg(Color::White))]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Other",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled("  /help   all slash commands", Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  Ctrl+R  session browser", Style::default().fg(Color::White))]),
    ];

    Paragraph::new(left_lines)
        .wrap(Wrap { trim: false })
        .render(cols[0], frame.buffer_mut());

    Paragraph::new(right_lines)
        .wrap(Wrap { trim: false })
        .render(cols[1], frame.buffer_mut());

    // Footer at the bottom of the inner area
    let footer_area = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(2),
        width: inner.width,
        height: 2,
    };
    let footer = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            "  Page 2/2",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(vec![Span::styled(
            "  Enter: done  ·  ← : back  ·  Esc: close",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )]),
    ]);
    footer.render(footer_area, frame.buffer_mut());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn onboarding_defaults_hidden() {
        let state = OnboardingDialogState::new();
        assert!(!state.visible);
        assert_eq!(state.page, OnboardingPage::Welcome);
    }

    #[test]
    fn onboarding_show_sets_visible() {
        let mut state = OnboardingDialogState::new();
        state.show();
        assert!(state.visible);
        assert_eq!(state.page, OnboardingPage::Welcome);
    }

    #[test]
    fn onboarding_next_page_cycles() {
        let mut state = OnboardingDialogState::new();
        state.show();
        assert!(!state.next_page()); // Welcome → KeyBindings
        assert_eq!(state.page, OnboardingPage::KeyBindings);
        assert!(state.next_page()); // KeyBindings → Done
        assert_eq!(state.page, OnboardingPage::Done);
        assert!(state.is_done());
    }

    #[test]
    fn onboarding_prev_page() {
        let mut state = OnboardingDialogState::new();
        state.show();
        state.next_page();
        state.prev_page();
        assert_eq!(state.page, OnboardingPage::Welcome);
    }

    #[test]
    fn onboarding_renders_without_panic() {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let mut state = OnboardingDialogState::new();
        state.show();
        terminal.draw(|frame| {
            render_onboarding_dialog(frame, &state, frame.area());
        }).unwrap();
        let content: String = terminal.backend().buffer().clone().content().iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Welcome") || content.contains("Claude"));
    }

    #[test]
    fn onboarding_keybindings_page_renders() {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let mut state = OnboardingDialogState::new();
        state.show();
        state.next_page();
        terminal.draw(|frame| {
            render_onboarding_dialog(frame, &state, frame.area());
        }).unwrap();
        let content: String = terminal.backend().buffer().clone().content().iter()
            .map(|c| c.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Keyboard") || content.contains("Enter"));
    }

    #[test]
    fn onboarding_hidden_renders_nothing() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let state = OnboardingDialogState::new(); // visible = false
        let before = terminal.backend().buffer().clone();
        terminal.draw(|frame| {
            render_onboarding_dialog(frame, &state, frame.area());
        }).unwrap();
        assert_eq!(terminal.backend().buffer().content(), before.content());
    }
}

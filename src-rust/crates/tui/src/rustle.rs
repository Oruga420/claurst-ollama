//! Rustle mascot rendering for ratatui.
//!
//! A 3-row Unicode block-art creature. Call `rustle_lines()` to get 4 `Line`
//! values (3 body rows + 1 blank spacing row) ready for embedding in a Paragraph.
//!
//! Structure (top to bottom):
//!   Row 1 — head: narrow top widening downward (▄ creates the taper)
//!   Row 2 — claws + eyes: widest row, pincers extend from sides (▄ = gap-to-arm)
//!   Row 3 — body + legs: body tapers into four legs via ▀ gap
//!
//! Visual (Default pose):
//! ```text
//!  ▄██████▄       head (6 wide top, 8 wide bottom)
//! █▄██ █ ██▄█     claws + eyes (11 wide, widest)
//!  ███▀▀███       body→legs (8 wide body, 3+3 legs)
//! ```

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// The pose / expression of the Rustle mascot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustlePose {
    Default,
    ArmsUp,
    LookLeft,
    LookRight,
}

/// Body-part style: bold pink foreground (#e91e63).
fn body_style() -> Style {
    Style::default()
        .fg(Color::Rgb(233, 30, 99))
        .add_modifier(Modifier::BOLD)
}

/// Eye-row style: pink text on black background.
fn eye_bg_style() -> Style {
    Style::default()
        .fg(Color::Rgb(233, 30, 99))
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD)
}

/// Returns 4 Lines representing the Rustle mascot:
///   [0] — head row (narrow top widening downward)
///   [1] — claws + eyes row (widest — pincers extend from sides)
///   [2] — body + legs row (body tapers into four legs)
///   [3] — blank spacing line
pub fn rustle_lines(pose: &RustlePose) -> [Line<'static>; 4] {
    // Pose varies the claw row (Row 2):
    //   r2l — left claw + head edge (body_style)
    //   r2e — eye section           (eye_bg_style)
    //   r2r — head edge + right claw (body_style)
    // Head (Row 1) and legs (Row 3) are fixed across poses.

    let (r2l, r2e, r2r) = match pose {
        RustlePose::Default => (
            "█▄██",     // left claw tip, ▄ gap-to-connect, head edges
            " █ ",      // centered eyes (space = black pupil, █ = bridge)
            "██▄█",     // head edges, ▄ connect-to-gap, right claw tip
        ),
        RustlePose::ArmsUp => (
            "█▀██",     // ▀ = claw raised (upper half = arm up)
            " █ ",
            "██▀█",     // raised right claw
        ),
        RustlePose::LookLeft => (
            "█▄██",
            "▐█▐",     // pupils shifted left (▐ = right-half pink, left-half black)
            "██▄█",
        ),
        RustlePose::LookRight => (
            "█▄██",
            "▌█▌",     // pupils shifted right (▌ = left-half pink, right-half black)
            "██▄█",
        ),
    };

    // Row 1: head — narrow top, wider bottom
    // Upper half: ` ██████ ` (6 wide), Lower half: `████████` (8 wide)
    let row1 = Line::from(vec![
        Span::styled(" ▄██████▄ ".to_string(), body_style()),
    ]);

    // Row 2: claws extending from sides + face with eyes (widest row)
    let row2 = Line::from(vec![
        Span::styled(r2l.to_string(), body_style()),
        Span::styled(r2e.to_string(), eye_bg_style()),
        Span::styled(r2r.to_string(), body_style()),
    ]);

    // Row 3: body tapering into four legs
    // Upper half: `████████` (8 wide body), Lower half: `███  ███` (3+2gap+3 legs)
    let row3 = Line::from(vec![
        Span::styled(" ███▀▀███ ".to_string(), body_style()),
    ]);

    // Row 4: blank spacing
    let row4 = Line::from("");

    [row1, row2, row3, row4]
}

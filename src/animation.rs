// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Terminal animation for one "hero" section of a rendered human report —
//! originally just Fract's section in `uni analyze`'s report, now the same
//! deterministic wave reveal shared by every subcommand with a flagship
//! table or section worth drawing attention to (`uni analyze`'s Fract
//! section, `uni revise`'s remediation list, `uni experiments`'s ranking
//! table, `uni --cohort`'s repo table).
//!
//! The last frame is always the original section, and non-interactive or
//! automation output stays static.

use std::io::{self, IsTerminal, Write as _};
use std::ops::Range;
use std::time::Duration;

use crate::render::HumanReport;

const FRAME_DELAY: Duration = Duration::from_millis(55);
const MAX_FRAMES: usize = 18;
const WAVE_FRONT_WIDTH: f64 = 0.16;
const SHARDS: [char; 8] = ['◇', '◈', '◆', '⋄', '✧', '✦', '⋆', '░'];

#[derive(Clone)]
enum Token {
    Ansi(String),
    Char(char),
}

/// Wave-reveals `report.fract_section` of `uni analyze`'s output. Thin
/// wrapper over [`present_text`] kept for its existing call site and
/// `HumanReport`'s existing shape.
pub async fn present(report: &HumanReport) -> io::Result<()> {
    present_text(&report.text, report.fract_section.clone()).await
}

/// Wave-reveals `reveal` (byte range into `full_text`) before settling on
/// the plain, final render — or writes `full_text` straight out, unanimated,
/// when `reveal` is `None`, motion is disabled (piped output, `NO_COLOR`,
/// a dumb terminal, or `CI`), or the section is too small/empty to animate.
/// Every subcommand's human renderer picks its own `reveal` range the same
/// way `render::technical_report` picks Fract's: record `out.len()` before
/// writing the section, then again after.
pub async fn present_text(full_text: &str, reveal: Option<Range<usize>>) -> io::Result<()> {
    let Some(section) = reveal.filter(|_| animation_enabled()) else {
        return write_static(full_text);
    };

    let prefix = &full_text[..section.start];
    let body = &full_text[section.clone()];
    let suffix = &full_text[section.end..];
    let frames = wave_frames(body, MAX_FRAMES);
    if frames.len() == 1 {
        return write_static(full_text);
    }

    let mut stdout = io::stdout().lock();
    stdout.write_all(prefix.as_bytes())?;
    stdout.write_all(form3::ansi::save_cursor().as_bytes())?;
    for frame in frames.iter().take(frames.len() - 1) {
        repaint(&mut stdout, frame)?;
        stdout.flush()?;
        tokio::time::sleep(FRAME_DELAY).await;
        stdout.write_all(form3::ansi::restore_cursor().as_bytes())?;
        stdout.write_all(form3::ansi::clear_to_end_of_screen().as_bytes())?;
    }
    repaint(&mut stdout, body)?;
    if body.ends_with('\n') {
        stdout.write_all(b"\n")?;
    }
    stdout.write_all(suffix.as_bytes())?;
    stdout.flush()
}

fn animation_enabled() -> bool {
    animation_enabled_for(
        io::stdout().is_terminal(),
        matches!(std::env::var("TERM").as_deref(), Ok("dumb")),
        std::env::var_os("CI").is_some(),
    )
}

fn animation_enabled_for(is_terminal: bool, dumb_terminal: bool, in_ci: bool) -> bool {
    is_terminal && !dumb_terminal && !in_ci
}

fn write_static(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()
}

fn wave_frames(text: &str, max_frames: usize) -> Vec<String> {
    let lines: Vec<Vec<Token>> = text.lines().map(tokenize_ansi).collect();
    let max_width = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|token| match token {
                    Token::Ansi(_) => 0,
                    Token::Char(ch) => char_width(*ch),
                })
                .sum()
        })
        .max();
    let max_width = match max_width {
        Some(width) => width,
        None => 0,
    };
    if text.is_empty() || max_width == 0 || max_frames < 2 {
        return vec![text.to_string()];
    }

    let frame_count = max_frames.saturating_sub(1).min(MAX_FRAMES);
    let center_x = max_width.saturating_sub(1) as f64 / 2.0;
    let center_y = lines.len().saturating_sub(1) as f64 / 2.0;
    let max_distance = center_x.hypot(center_y).max(1.0);
    let mut frames = Vec::with_capacity(frame_count + 1);

    for frame in 0..frame_count {
        let progress = frame as f64 / frame_count.saturating_sub(1).max(1) as f64;
        let mut output = String::new();
        for (row, line) in lines.iter().enumerate() {
            if row > 0 {
                output.push('\n');
            }
            let mut column = 0usize;
            for token in line {
                match token {
                    Token::Ansi(sequence) => output.push_str(sequence),
                    Token::Char(ch) => {
                        let width = char_width(*ch);
                        let distance = (column as f64 + width as f64 / 2.0 - center_x)
                            .hypot(row as f64 - center_y)
                            / max_distance;
                        if ch.is_whitespace() || width == 0 || distance <= progress {
                            output.push(*ch);
                        } else if distance <= progress + WAVE_FRONT_WIDTH {
                            output.push(
                                SHARDS[(row.wrapping_mul(31) + column.wrapping_mul(17))
                                    % SHARDS.len()],
                            );
                            output.push_str(&" ".repeat(width.saturating_sub(1)));
                        } else {
                            output.push_str(&" ".repeat(width));
                        }
                        column += width;
                    }
                }
            }
            output.push_str(&" ".repeat(max_width.saturating_sub(column)));
        }
        frames.push(output);
    }
    frames.push(text.to_string());
    frames
}

fn tokenize_ansi(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            let mut sequence = String::from(ch);
            if let Some(introducer) = chars.next() {
                sequence.push(introducer);
            }
            while let Some(part) = chars.next() {
                sequence.push(part);
                if ('@'..='~').contains(&part) {
                    break;
                }
            }
            tokens.push(Token::Ansi(sequence));
        } else {
            tokens.push(Token::Char(ch));
        }
    }
    tokens
}

/// Delegates to `form3::width`'s vendored Unicode-width tables instead of
/// hand-picking combining/wide code-point ranges here — one Unicode-width
/// engine for the whole `uni`/`3form` suite instead of a second one just for
/// Fract's wave reveal.
fn char_width(ch: char) -> usize {
    form3::width::display_width(&ch.to_string())
}

fn repaint(writer: &mut impl io::Write, text: &str) -> io::Result<()> {
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            writer.write_all(b"\n")?;
        }
        writer.write_all(form3::ansi::clear_line().as_bytes())?;
        writer.write_all(line.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_finishes_with_exact_original_section() {
        let text = "  ✅ fract: healthy\n      • src/main.rs — cohesive\n";
        let frames = wave_frames(text, 12);
        assert!(frames.len() > 2);
        assert_eq!(frames.last().map(String::as_str), Some(text));
        assert!(SHARDS.iter().any(|shard| frames[0].contains(*shard)));
    }

    #[test]
    fn wave_preserves_ansi_sequences_without_counting_them_as_width() {
        let text = "\x1b[33m⚠ warning\x1b[0m\n";
        let frames = wave_frames(text, 12);
        assert_eq!(frames.last().map(String::as_str), Some(text));
        assert!(frames.iter().all(|frame| frame.contains("\x1b[33m")));
        assert!(frames.iter().all(|frame| frame.contains("\x1b[0m")));
    }

    #[test]
    fn automation_and_incompatible_terminals_disable_motion() {
        assert!(animation_enabled_for(true, false, false));
        assert!(!animation_enabled_for(false, false, false));
        assert!(!animation_enabled_for(true, true, false));
        assert!(!animation_enabled_for(true, false, true));
    }

    #[test]
    fn display_width_handles_combining_and_wide_characters() {
        assert_eq!("e\u{301}".chars().map(char_width).sum::<usize>(), 1);
        assert_eq!(char_width('界'), 2);
        assert_eq!(char_width('✅'), 2);
    }
}

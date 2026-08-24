// Copyright (c) 2026 sal
// SPDX-License-Identifier: MIT
//! Terminal animation for Fract's section in the unified human report.
//!
//! This mirrors Fract's deterministic wave reveal. The last frame is always
//! the original section, and non-interactive or automation output stays static.

use std::io::{self, IsTerminal, Write as _};
use std::time::Duration;

use crate::render::HumanReport;

const FRAME_DELAY: Duration = Duration::from_millis(55);
const MAX_FRAMES: usize = 18;
const WAVE_FRONT_WIDTH: f64 = 0.16;
const SHARDS: [char; 8] = ['◇', '◈', '◆', '⋄', '✧', '✦', '⋆', '░'];

pub async fn present(report: &HumanReport) -> io::Result<()> {
    let Some(section) = report.fract_section.clone().filter(|_| animation_enabled()) else {
        return write_static(&report.text);
    };

    let prefix = &report.text[..section.start];
    let body = &report.text[section.clone()];
    let suffix = &report.text[section.end..];
    let frames = wave_frames(body, MAX_FRAMES);
    if frames.len() == 1 {
        return write_static(&report.text);
    }

    let height = body.lines().count().max(1);
    let mut stdout = io::stdout().lock();
    stdout.write_all(prefix.as_bytes())?;
    for frame in frames.iter().take(frames.len() - 1) {
        repaint(&mut stdout, frame)?;
        stdout.flush()?;
        tokio::time::sleep(FRAME_DELAY).await;
        rewind(&mut stdout, height)?;
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
    let lines: Vec<Vec<char>> = text.lines().map(|line| line.chars().collect()).collect();
    let max_width = lines
        .iter()
        .map(|line| line.iter().map(|ch| char_width(*ch)).sum())
        .max()
        .unwrap_or(0);
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
            for ch in line {
                let width = char_width(*ch);
                let distance = (column as f64 + width as f64 / 2.0 - center_x)
                    .hypot(row as f64 - center_y)
                    / max_distance;
                if ch.is_whitespace() || width == 0 || distance <= progress {
                    output.push(*ch);
                } else if distance <= progress + WAVE_FRONT_WIDTH {
                    output.push(
                        SHARDS[(row.wrapping_mul(31) + column.wrapping_mul(17)) % SHARDS.len()],
                    );
                    output.push_str(&" ".repeat(width.saturating_sub(1)));
                } else {
                    output.push_str(&" ".repeat(width));
                }
                column += width;
            }
            output.push_str(&" ".repeat(max_width.saturating_sub(column)));
        }
        frames.push(output);
    }
    frames.push(text.to_string());
    frames
}

fn char_width(ch: char) -> usize {
    let code = u32::from(ch);
    if ch.is_control() || is_combining(code) {
        0
    } else if is_wide(code) {
        2
    } else {
        1
    }
}

fn is_combining(code: u32) -> bool {
    matches!(
        code,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF
            | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F | 0xE0100..=0xE01EF
    )
}

fn is_wide(code: u32) -> bool {
    matches!(
        code,
        0x1100..=0x115F | 0x2329..=0x232A | 0x2600..=0x27BF | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF | 0xFE10..=0xFE19 | 0xFE30..=0xFE6F | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD
    )
}

fn repaint(writer: &mut impl io::Write, text: &str) -> io::Result<()> {
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            writer.write_all(b"\n")?;
        }
        writer.write_all(b"\r\x1b[2K")?;
        writer.write_all(line.as_bytes())?;
    }
    Ok(())
}

fn rewind(writer: &mut impl io::Write, height: usize) -> io::Result<()> {
    if height > 1 {
        write!(writer, "\x1b[{}A", height - 1)?;
    }
    writer.write_all(b"\r")
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

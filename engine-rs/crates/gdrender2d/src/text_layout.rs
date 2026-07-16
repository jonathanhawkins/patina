//! Pure text-layout primitives for the 2D TextServer (pat-nzgdu.91).
//!
//! Godot lays out text through `TextServer`/`TextParagraph`/`TextLine`: breaking
//! lines by word against a width, aligning them, trimming with an ellipsis on
//! overrun, and advancing tabs to tab stops. This module provides those core
//! computations over precomputed word/glyph advances (f32 pixels), independent
//! of the font backend — the paragraph/Label render path supplies the metrics
//! and consumes the resulting line ranges and offsets.

/// Horizontal alignment of a line within its bounding width
/// (Godot `HorizontalAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    /// Flush to the left edge.
    Left,
    /// Centered within the width.
    Center,
    /// Flush to the right edge.
    Right,
    /// Justified (spaces expand); the line's start offset is `0`.
    Fill,
}

/// A run of words `[start, end)` (indices into the word list) forming one line,
/// with the line's total content width in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineRange {
    /// Index of the first word on the line.
    pub start: usize,
    /// One past the index of the last word on the line.
    pub end: usize,
    /// Total width of the line's words plus inter-word spaces, in pixels.
    pub width: f32,
}

/// Total width of `word_widths` joined by `space_width` between adjacent words.
fn joined_width(word_widths: &[f32], space_width: f32) -> f32 {
    if word_widths.is_empty() {
        return 0.0;
    }
    let sum: f32 = word_widths.iter().sum();
    sum + space_width * (word_widths.len() as f32 - 1.0)
}

/// Greedily breaks a sequence of word advances into lines that each fit within
/// `max_width`, matching Godot's `AUTOWRAP_WORD`. Words are separated by
/// `space_width`. A single word wider than `max_width` occupies its own line
/// (word mode never breaks inside a word). `max_width <= 0` disables wrapping
/// (all words on one line).
pub fn break_lines_word(word_widths: &[f32], space_width: f32, max_width: f32) -> Vec<LineRange> {
    let mut lines = Vec::new();
    if word_widths.is_empty() {
        return lines;
    }
    if max_width <= 0.0 {
        lines.push(LineRange {
            start: 0,
            end: word_widths.len(),
            width: joined_width(word_widths, space_width),
        });
        return lines;
    }
    let mut start = 0;
    let mut cur_width = 0.0f32;
    for (i, &w) in word_widths.iter().enumerate() {
        if i == start {
            // The first word on a line always goes on it, even if it overflows.
            cur_width = w;
        } else if cur_width + space_width + w > max_width {
            // Adding this word would overflow: break the line before it.
            lines.push(LineRange {
                start,
                end: i,
                width: cur_width,
            });
            start = i;
            cur_width = w;
        } else {
            cur_width += space_width + w;
        }
    }
    lines.push(LineRange {
        start,
        end: word_widths.len(),
        width: cur_width,
    });
    lines
}

/// The x-offset at which a line of `line_width` starts so it is aligned within
/// `max_width`. `Fill` justifies by expanding spaces, so its start offset is
/// `0`. Never returns a negative offset (an overflowing line starts at `0`).
pub fn align_offset(line_width: f32, max_width: f32, align: HAlign) -> f32 {
    match align {
        HAlign::Left | HAlign::Fill => 0.0,
        HAlign::Center => ((max_width - line_width) * 0.5).max(0.0),
        HAlign::Right => (max_width - line_width).max(0.0),
    }
}

/// For a single line that overflows `max_width`, returns how many leading words
/// fit before a trailing ellipsis of `ellipsis_width` (Godot
/// `OVERRUN_TRIM_ELLIPSIS`). Returns `None` when the whole line fits (draw no
/// ellipsis). May return `0` when not even the first word fits beside the
/// ellipsis.
pub fn ellipsis_word_count(
    word_widths: &[f32],
    space_width: f32,
    max_width: f32,
    ellipsis_width: f32,
) -> Option<usize> {
    if joined_width(word_widths, space_width) <= max_width {
        return None; // fits; no trimming
    }
    let budget = max_width - ellipsis_width;
    let mut count = 0;
    let mut w = 0.0f32;
    for (i, &ww) in word_widths.iter().enumerate() {
        let cand = if i == 0 { ww } else { w + space_width + ww };
        if cand <= budget {
            w = cand;
            count = i + 1;
        } else {
            break;
        }
    }
    Some(count)
}

/// Advances `x` to the next tab stop, a multiple of `tab_width` (Godot tab
/// stops). At a stop exactly, advances to the following one. A non-positive
/// `tab_width` leaves `x` unchanged.
pub fn tab_advance(x: f32, tab_width: f32) -> f32 {
    if tab_width <= 0.0 {
        return x;
    }
    ((x / tab_width).floor() + 1.0) * tab_width
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn word_wrap_breaks_when_next_word_overflows() {
        // Three 10px words, 2px spaces, 25px width:
        //   "w0 w1" = 10+2+10 = 22 <= 25; adding w2 -> 34 > 25 -> new line.
        let lines = break_lines_word(&[10.0, 10.0, 10.0], 2.0, 25.0);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].start, lines[0].end), (0, 2));
        assert!(approx(lines[0].width, 22.0));
        assert_eq!((lines[1].start, lines[1].end), (2, 3));
        assert!(approx(lines[1].width, 10.0));
    }

    #[test]
    fn oversized_single_word_gets_its_own_line() {
        let lines = break_lines_word(&[40.0, 5.0], 2.0, 20.0);
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].start, lines[0].end), (0, 1)); // 40 alone, overflowing
        assert_eq!((lines[1].start, lines[1].end), (1, 2));
    }

    #[test]
    fn alignment_offsets_place_line_within_width() {
        assert!(approx(align_offset(22.0, 30.0, HAlign::Left), 0.0));
        assert!(approx(align_offset(22.0, 30.0, HAlign::Center), 4.0));
        assert!(approx(align_offset(22.0, 30.0, HAlign::Right), 8.0));
        // Overflowing line never yields a negative offset.
        assert!(approx(align_offset(40.0, 30.0, HAlign::Right), 0.0));
    }

    #[test]
    fn ellipsis_keeps_leading_words_within_budget() {
        // full = 34 > 25; budget = 25 - 5 = 20; only "w0" (10) fits (w0 w1 = 22 > 20).
        assert_eq!(
            ellipsis_word_count(&[10.0, 10.0, 10.0], 2.0, 25.0, 5.0),
            Some(1)
        );
        // Everything fits -> no ellipsis.
        assert_eq!(ellipsis_word_count(&[5.0, 5.0], 2.0, 20.0, 5.0), None);
    }

    #[test]
    fn tab_advances_to_next_stop() {
        assert!(approx(tab_advance(0.0, 8.0), 8.0));
        assert!(approx(tab_advance(5.0, 8.0), 8.0));
        assert!(approx(tab_advance(8.0, 8.0), 16.0)); // at a stop -> next stop
        assert!(approx(tab_advance(10.0, 8.0), 16.0));
    }
}

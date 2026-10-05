//! ANSI SGR escape sequences to styled preview spans.
//!
//! An external previewer — `bat --color=always`, `glow`, `chafa` — colours its
//! output with SGR sequences (`ESC [ … m`). This module keeps exactly those, as
//! [`StyledSpan`] colours and modifiers, and discards every other sequence:
//! cursor movement, screen clears, OSC window titles and hyperlinks, and the
//! DCS/APC payloads that carry sixel and kitty images. Those would act on the
//! terminal rather than be drawn in the pane, which is the artifact
//! [`sanitize`] exists to prevent; what is left after parsing goes through it.
//!
//! Written here rather than taken from `ansi-to-tui`, whose releases each pin a
//! `ratatui` major — the two-ratatui trap in `CLAUDE.md` §9.

use ratatui::style::{Color, Modifier};

use crate::preview::provider::{sanitize, HighlightedLine, StyledSpan};

/// ESC, which opens every sequence this module recognises.
const ESC: u8 = 0x1b;

/// BEL, which terminates an OSC string as an alternative to `ESC \`.
const BEL: u8 = 0x07;

/// Form feed: a page break in tool output, dropped rather than drawn.
const FORM_FEED: u8 = 0x0c;

/// Parses `text` into lines of styled spans.
///
/// Lines are split on `\n`, with a `\r` before it dropped, so CRLF output from
/// Windows tools reads the same as LF. The style carries over line breaks, as
/// it does in a terminal. A trailing newline does not produce an empty last
/// line. A sequence cut off by the end of the input is dropped.
///
/// ```
/// use ratatui::style::{Color, Modifier};
/// use trail::preview::ansi::parse;
///
/// let lines = parse("plain \u{1b}[1;31mred\u{1b}[0m\n");
/// assert_eq!(lines.len(), 1);
/// assert_eq!(lines[0][0].text, "plain ");
/// assert_eq!(lines[0][1].text, "red");
/// assert_eq!(lines[0][1].fg, Some(Color::Red));
/// assert!(lines[0][1].modifiers.contains(Modifier::BOLD));
/// ```
pub fn parse(text: &str) -> Vec<HighlightedLine> {
    // Scanned by byte offset, so a run of plain text is copied out of the input
    // as one slice rather than char by char. Every sequence is ASCII, and each
    // offset this loop stops at is an ASCII byte or the start of a character,
    // which keeps every slice on a character boundary.
    let bytes = text.as_bytes();
    let mut out = Builder::default();
    let mut run_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            ESC => {
                out.text.push_str(&text[run_start..i]);
                i = skip_escape(text, i, &mut out);
                run_start = i;
            }
            b'\n' => {
                let run = &text[run_start..i];
                // CRLF reads as LF.
                out.text.push_str(run.strip_suffix('\r').unwrap_or(run));
                out.end_line();
                i += 1;
                run_start = i;
            }
            // A form feed is a page break — `pdftotext` writes one between
            // pages — not text, and drawn as a placeholder it would start every
            // page with a stray mark.
            FORM_FEED => {
                out.text.push_str(&text[run_start..i]);
                i += 1;
                run_start = i;
            }
            _ => i += 1,
        }
    }
    out.text.push_str(&text[run_start..]);

    out.finish()
}

/// Handles the escape sequence starting at `bytes[start]` (an ESC) and returns
/// the offset just past it — applying it to `out`'s pen if it is an SGR, and
/// discarding it otherwise.
///
/// A sequence cut off by the end of the input returns the input's length. A
/// string sequence (OSC, DCS…) ended by an ESC that is not `ESC \` returns
/// that ESC's offset, so it can start the next sequence.
fn skip_escape(text: &str, start: usize, out: &mut Builder) -> usize {
    let bytes = text.as_bytes();
    let end = bytes.len();
    let Some(&kind) = bytes.get(start + 1) else {
        return end;
    };
    match kind {
        b'[' => {
            let mut j = start + 2;
            while j < end && (0x20..=0x3f).contains(&bytes[j]) {
                j += 1;
            }
            match bytes.get(j) {
                Some(&fin) if (0x40..=0x7e).contains(&fin) => {
                    let params = &text[start + 2..j];
                    // Only a plain SGR. `ESC [ > 4 ; 2 m` ends in `m` too, but it
                    // is xterm's key-modifier setting, not a colour.
                    if fin == b'm'
                        && params
                            .bytes()
                            .all(|b| b.is_ascii_digit() || b == b';' || b == b':')
                    {
                        out.flush();
                        out.pen.apply_sgr(params);
                    }
                    j + 1
                }
                // Not part of a CSI: the sequence is malformed. Abandon it and
                // let the byte be handled normally.
                Some(_) => j,
                None => end,
            }
        }
        // OSC, DCS, SOS, PM, APC: a string running to its terminator.
        b']' | b'P' | b'X' | b'^' | b'_' => {
            let mut j = start + 2;
            while j < end {
                match bytes[j] {
                    BEL => return j + 1,
                    ESC if bytes.get(j + 1) == Some(&b'\\') => return j + 2,
                    ESC => return j,
                    _ => j += 1,
                }
            }
            end
        }
        // nF escapes such as `ESC ( B` (character set): intermediate bytes,
        // then one final byte.
        0x20..=0x2f => {
            let mut j = start + 2;
            while j < end && (0x20..=0x2f).contains(&bytes[j]) {
                j += 1;
            }
            match bytes.get(j) {
                Some(&fin) if fin.is_ascii() => j + 1,
                // Never step into the middle of a multi-byte character.
                Some(_) => j,
                None => end,
            }
        }
        // Two-character escapes (`ESC 7`, `ESC =`).
        _ if kind.is_ascii() => start + 2,
        // ESC before a multi-byte character: drop the ESC, keep the character.
        _ => start + 1,
    }
}

/// Most parameters one SGR sequence is read for; any past this are ignored.
/// Real output uses a handful — `38;2;r;g;b` plus a modifier or two.
const MAX_SGR_PARAMS: usize = 32;

/// The style the next printed character is drawn in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Pen {
    fg: Option<Color>,
    bg: Option<Color>,
    modifiers: Modifier,
}

impl Pen {
    /// Applies the parameters of one SGR sequence — the text between `ESC [`
    /// and `m`.
    ///
    /// Parameters are separated by `;`; a `:` separates the sub-parameters of
    /// one parameter, which is how `38:2::r:g:b` keeps its colour together.
    /// An empty parameter list, like an explicit `0`, resets everything.
    fn apply_sgr(&mut self, params: &str) {
        // Flattened into fixed arrays rather than a vector per group: a
        // coloured line carries a dozen of these, and allocating for each one
        // was most of the parser's time. `starts[g]..starts[g + 1]` is group g.
        let mut vals = [None::<u16>; MAX_SGR_PARAMS];
        let mut starts = [0usize; MAX_SGR_PARAMS + 1];
        let mut n = 0;
        let mut groups = 0;
        for group in params.split(';') {
            if n == MAX_SGR_PARAMS {
                break;
            }
            starts[groups] = n;
            groups += 1;
            for sub in group.split(':').take(MAX_SGR_PARAMS - n) {
                vals[n] = sub.parse().ok();
                n += 1;
            }
        }
        starts[groups] = n;

        let mut i = 0;
        while i < groups {
            let group = &vals[starts[i]..starts[i + 1]];
            let code = group.first().copied().flatten().unwrap_or(0);
            match code {
                0 => *self = Pen::default(),
                1 => self.modifiers.insert(Modifier::BOLD),
                2 => self.modifiers.insert(Modifier::DIM),
                3 => self.modifiers.insert(Modifier::ITALIC),
                // `4:0` is "no underline" in the sub-parameter form; any other
                // underline style is drawn as the one underline a cell has.
                4 if group.get(1) == Some(&Some(0)) => self.modifiers.remove(Modifier::UNDERLINED),
                4 => self.modifiers.insert(Modifier::UNDERLINED),
                5 => self.modifiers.insert(Modifier::SLOW_BLINK),
                6 => self.modifiers.insert(Modifier::RAPID_BLINK),
                7 => self.modifiers.insert(Modifier::REVERSED),
                8 => self.modifiers.insert(Modifier::HIDDEN),
                9 => self.modifiers.insert(Modifier::CROSSED_OUT),
                22 => self.modifiers.remove(Modifier::BOLD | Modifier::DIM),
                23 => self.modifiers.remove(Modifier::ITALIC),
                24 => self.modifiers.remove(Modifier::UNDERLINED),
                25 => self
                    .modifiers
                    .remove(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK),
                27 => self.modifiers.remove(Modifier::REVERSED),
                28 => self.modifiers.remove(Modifier::HIDDEN),
                29 => self.modifiers.remove(Modifier::CROSSED_OUT),
                30..=37 => self.fg = Some(basic_color(code - 30)),
                39 => self.fg = None,
                40..=47 => self.bg = Some(basic_color(code - 40)),
                49 => self.bg = None,
                90..=97 => self.fg = Some(basic_color(code - 90 + 8)),
                100..=107 => self.bg = Some(basic_color(code - 100 + 8)),
                // Extended colour: foreground, background, and underline colour,
                // which a cell cannot show but whose arguments must be skipped.
                38 | 48 | 58 => {
                    let (color, consumed) = if group.len() > 1 {
                        (extended_color(&group[1..]), 0)
                    } else {
                        // The colour's arguments are the next groups, at most
                        // four of them (`2;r;g;b`).
                        let mut rest = [None::<u16>; 4];
                        let available = (groups - i - 1).min(rest.len());
                        for (k, slot) in rest.iter_mut().take(available).enumerate() {
                            *slot = vals[starts[i + 1 + k]];
                        }
                        extended_color_semicolons(&rest[..available])
                    };
                    match code {
                        38 => self.fg = color.or(self.fg),
                        48 => self.bg = color.or(self.bg),
                        _ => {}
                    }
                    i += consumed;
                }
                // 21 (double underline, or bold-off on some terminals), fonts,
                // frames, overlines: nothing a cell can show differently.
                _ => {}
            }
            i += 1;
        }
    }
}

/// The colour for one of the sixteen basic palette slots: 0–7 normal, 8–15
/// bright.
fn basic_color(index: u16) -> Color {
    match index {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        _ => Color::White,
    }
}

/// Reads a colon-form extended colour: the sub-parameters after `38`/`48`.
///
/// `5:n` is a 256-colour index; `2:r:g:b`, or `2:id:r:g:b` with a colour-space
/// id (usually left empty), is 24-bit.
fn extended_color(sub: &[Option<u16>]) -> Option<Color> {
    match sub.first().copied().flatten()? {
        5 => Some(Color::Indexed(byte(sub.get(1)?)?)),
        2 => {
            let rgb = if sub.len() >= 5 {
                &sub[2..5]
            } else {
                sub.get(1..4)?
            };
            Some(Color::Rgb(byte(&rgb[0])?, byte(&rgb[1])?, byte(&rgb[2])?))
        }
        _ => None,
    }
}

/// Reads a semicolon-form extended colour from the parameters after
/// `38`/`48`, returning the colour and how many parameters it used.
///
/// The count is returned even when the colour is malformed, so the caller
/// skips the arguments instead of reading `2;255;0;0` as "dim" and three
/// unknown codes.
fn extended_color_semicolons(rest: &[Option<u16>]) -> (Option<Color>, usize) {
    match rest.first().copied().flatten() {
        Some(5) => {
            let color = rest.get(1).and_then(byte).map(Color::Indexed);
            (color, 2.min(rest.len()))
        }
        Some(2) => {
            let color = match rest.get(1..4) {
                Some([r, g, b]) => byte(r)
                    .zip(byte(g))
                    .zip(byte(b))
                    .map(|((r, g), b)| Color::Rgb(r, g, b)),
                _ => None,
            };
            (color, 4.min(rest.len()))
        }
        _ => (None, 0),
    }
}

/// A parameter as a colour component, or `None` when it is absent or past 255.
fn byte(value: &Option<u16>) -> Option<u8> {
    value.and_then(|v| u8::try_from(v).ok())
}

/// Accumulates spans and lines as the parser walks the input.
#[derive(Default)]
struct Builder {
    lines: Vec<HighlightedLine>,
    line: HighlightedLine,
    /// Printable text waiting for the pen to change, or the line to end.
    text: String,
    pen: Pen,
}

impl Builder {
    /// Closes the pending text into a span in the current pen.
    fn flush(&mut self) {
        if self.text.is_empty() {
            return;
        }
        // Most runs contain nothing to defuse; only those that do pay for a copy.
        let text = if self.text.chars().any(char::is_control) {
            let safe = sanitize(&self.text);
            self.text.clear();
            safe
        } else {
            std::mem::take(&mut self.text)
        };
        // Redundant SGRs (`ESC[31m` twice) would otherwise split one run.
        if let Some(last) = self.line.last_mut() {
            if last.fg == self.pen.fg
                && last.bg == self.pen.bg
                && last.modifiers == self.pen.modifiers
            {
                last.text.push_str(&text);
                return;
            }
        }
        self.line.push(StyledSpan {
            text,
            fg: self.pen.fg,
            bg: self.pen.bg,
            modifiers: self.pen.modifiers,
        });
    }

    fn end_line(&mut self) {
        self.flush();
        self.lines.push(std::mem::take(&mut self.line));
    }

    fn finish(mut self) -> Vec<HighlightedLine> {
        self.flush();
        if !self.line.is_empty() {
            self.lines.push(self.line);
        }
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of each line, spans joined — for asserting on content alone.
    fn texts(lines: &[HighlightedLine]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    fn only_span(text: &str) -> StyledSpan {
        let lines = parse(text);
        assert_eq!(lines.len(), 1, "one line expected from {text:?}");
        assert_eq!(lines[0].len(), 1, "one span expected from {text:?}");
        lines[0][0].clone()
    }

    #[test]
    fn plain_text_is_one_unstyled_span_per_line() {
        let lines = parse("one\ntwo\n");
        assert_eq!(texts(&lines), ["one", "two"]);
        assert_eq!(lines[0][0].fg, None);
        assert_eq!(lines[0][0].bg, None);
        assert_eq!(lines[0][0].modifiers, Modifier::empty());
    }

    #[test]
    fn basic_and_bright_colours() {
        assert_eq!(only_span("\u{1b}[31mx").fg, Some(Color::Red));
        assert_eq!(only_span("\u{1b}[44mx").bg, Some(Color::Blue));
        assert_eq!(only_span("\u{1b}[92mx").fg, Some(Color::LightGreen));
        assert_eq!(only_span("\u{1b}[107mx").bg, Some(Color::White));
    }

    #[test]
    fn indexed_colours_in_both_separator_forms() {
        assert_eq!(only_span("\u{1b}[38;5;208mx").fg, Some(Color::Indexed(208)));
        assert_eq!(only_span("\u{1b}[48:5:17mx").bg, Some(Color::Indexed(17)));
    }

    #[test]
    fn truecolour_in_all_three_spellings() {
        let orange = Some(Color::Rgb(255, 128, 0));
        assert_eq!(only_span("\u{1b}[38;2;255;128;0mx").fg, orange);
        assert_eq!(only_span("\u{1b}[38:2:255:128:0mx").fg, orange);
        // With the (empty) colour-space id that the standard puts first.
        assert_eq!(only_span("\u{1b}[48:2::255:128:0mx").bg, orange);
    }

    /// `38;2;r;g;b` uses four parameters after the 38; skipping the wrong number
    /// would read the colour components as SGR codes — `2` is dim.
    #[test]
    fn an_extended_colour_consumes_its_arguments() {
        let span = only_span("\u{1b}[38;2;1;2;3;1mx");
        assert_eq!(span.fg, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(
            span.modifiers,
            Modifier::BOLD,
            "only the trailing 1 applies"
        );
    }

    #[test]
    fn modifiers_and_their_individual_resets() {
        let span = only_span("\u{1b}[1;3;4;7mx");
        assert!(span.modifiers.contains(
            Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED | Modifier::REVERSED
        ));

        let span = only_span("\u{1b}[1;3;4;7m\u{1b}[22;23;24;27mx");
        assert_eq!(span.modifiers, Modifier::empty());
    }

    #[test]
    fn reset_clears_colour_and_modifiers() {
        let lines = parse("\u{1b}[1;31;44mred\u{1b}[0mplain\u{1b}[31m\u{1b}[mplain2");
        let line = &lines[0];
        assert_eq!(line[0].text, "red");
        assert_eq!(
            line[1].text, "plainplain2",
            "`ESC[m` is a reset too, so the runs merge"
        );
        assert_eq!(line[1].fg, None);
        assert_eq!(line[1].bg, None);
        assert_eq!(line[1].modifiers, Modifier::empty());
    }

    #[test]
    fn default_colour_codes_clear_one_side_only() {
        let lines = parse("\u{1b}[31;44mab\u{1b}[39mcd\u{1b}[49mef");
        let line = &lines[0];
        assert_eq!((line[1].fg, line[1].bg), (None, Some(Color::Blue)));
        assert_eq!((line[2].fg, line[2].bg), (None, None));
    }

    #[test]
    fn style_carries_over_a_line_break() {
        let lines = parse("\u{1b}[32mone\ntwo");
        assert_eq!(lines[1][0].fg, Some(Color::Green));
    }

    /// Cursor movement, clears, titles, hyperlinks and image payloads would
    /// act on the terminal rather than be drawn. None may survive.
    #[test]
    fn non_sgr_sequences_are_discarded() {
        let input = concat!(
            "a\u{1b}[2J\u{1b}[H\u{1b}[10;5Hb", // clear, cursor moves
            "\u{1b}]0;window title\u{7}c",     // OSC title, BEL-terminated
            "\u{1b}]8;;https://x.test\u{1b}\\link\u{1b}]8;;\u{1b}\\", // OSC 8 hyperlink
            "\u{1b}Pq#0;2;0;0;0~-\u{1b}\\d",   // sixel DCS
            "\u{1b}_Ga=T,f=100;AAAA\u{1b}\\e", // kitty APC
            "\u{1b}(B\u{1b}7\u{1b}=f",         // charset, save cursor, keypad
            "\u{1b}[?25l\u{1b}[>4;2mg",        // private modes, not SGR
        );
        let lines = parse(input);
        assert_eq!(texts(&lines), ["abclinkdefg"]);
        assert!(lines[0]
            .iter()
            .all(|s| s.fg.is_none() && s.modifiers.is_empty()));
    }

    #[test]
    fn crlf_endings_read_as_lf_and_tabs_expand() {
        let lines = parse("one\r\n\ttwo\r\n");
        assert_eq!(texts(&lines), ["one", "    two"]);
    }

    /// A lone CR would send the cursor to column 0 mid-pane; it is drawn as a
    /// placeholder, like any other control character.
    #[test]
    fn a_bare_carriage_return_is_defused() {
        assert_eq!(texts(&parse("50%\r100%")), ["50%·100%"]);
    }

    #[test]
    fn a_sequence_cut_off_at_the_end_is_dropped() {
        assert_eq!(texts(&parse("text\u{1b}[38;2;25")), ["text"]);
        assert_eq!(texts(&parse("text\u{1b}")), ["text"]);
        assert_eq!(texts(&parse("text\u{1b}]0;never ends")), ["text"]);
    }

    /// An OSC cut short by a new escape hands that escape back, so the colour
    /// that follows still applies.
    #[test]
    fn an_unterminated_string_gives_way_to_the_next_sequence() {
        let lines = parse("\u{1b}]0;title\u{1b}[31mred");
        assert_eq!(texts(&lines), ["red"]);
        assert_eq!(lines[0][0].fg, Some(Color::Red));
    }

    #[test]
    fn empty_lines_are_kept_and_a_trailing_newline_adds_none() {
        assert_eq!(texts(&parse("a\n\nb\n")), ["a", "", "b"]);
        assert!(parse("").is_empty());
    }

    /// The scan works on byte offsets; a sequence next to a multi-byte
    /// character must never leave an offset inside it.
    #[test]
    fn escapes_next_to_multibyte_characters_keep_them_whole() {
        assert_eq!(texts(&parse("héllo\u{1b}[31m—wörld")), ["héllo—wörld"]);
        // ESC directly before a multi-byte character: the ESC goes, it stays.
        assert_eq!(texts(&parse("a\u{1b}éb")), ["aéb"]);
        // A malformed CSI and an nF escape cut short by one.
        assert_eq!(texts(&parse("a\u{1b}[12éb")), ["aéb"]);
        assert_eq!(texts(&parse("a\u{1b}(éb")), ["aéb"]);
        assert_eq!(texts(&parse("日本\u{1b}]0;題\u{7}語")), ["日本語"]);
    }

    /// `pdftotext` separates pages with a form feed, and a scanned PDF with no
    /// text layer produces nothing else — which has to read as empty, so the
    /// worker can say the tool produced no output.
    #[test]
    fn form_feeds_are_page_breaks_not_text() {
        assert_eq!(
            texts(&parse("page one\n\u{c}page two\n")),
            ["page one", "page two"]
        );
        assert!(texts(&parse("\u{c}\n\u{c}\n\u{c}"))
            .iter()
            .all(|l| l.trim().is_empty()));
    }

    /// More parameters than the parser reads are ignored, not a crash.
    #[test]
    fn an_overlong_sgr_is_read_up_to_its_limit() {
        let many = vec!["1"; 100].join(";");
        let span = only_span(&format!("\u{1b}[{many};31mx"));
        assert!(span.modifiers.contains(Modifier::BOLD));
    }

    #[test]
    fn an_out_of_range_colour_is_ignored_not_wrapped() {
        assert_eq!(only_span("\u{1b}[38;5;300mx").fg, None);
        assert_eq!(only_span("\u{1b}[38;2;256;0;0mx").fg, None);
    }
}

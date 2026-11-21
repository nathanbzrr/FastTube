//! Safe, bounded WebVTT parsing and transcript search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

fn is_vtt_metadata_block(line: &str) -> bool {
    ["STYLE", "NOTE", "REGION"].iter().any(|keyword| {
        line.strip_prefix(keyword)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
    })
}

pub fn parse_vtt(input: &str) -> Result<Vec<Cue>, &'static str> {
    if input.len() > 2 * 1024 * 1024 {
        return Err("legenda excede o limite de 2 MiB");
    }
    let mut cues: Vec<Cue> = Vec::new();
    let mut last_cue_bottom_line: Option<String> = None;
    let mut lines = input.lines().peekable();
    while let Some(line) = lines.next() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if is_vtt_metadata_block(line) {
            for next in lines.by_ref() {
                if next.trim().is_empty() {
                    break;
                }
            }
            continue;
        }
        if !line.contains("-->") {
            continue;
        }
        let Some((start, tail)) = line.split_once("-->") else {
            continue;
        };
        let end = tail.split_whitespace().next().unwrap_or("");
        let (Some(start_ms), Some(end_ms)) = (timestamp_ms(start.trim()), timestamp_ms(end.trim()))
        else {
            continue;
        };
        if end_ms < start_ms {
            continue;
        }
        let mut cue_lines = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                lines.next();
                break;
            }
            if next.contains("-->") {
                break;
            }
            if let Some(cue_line) = lines.next() {
                let cleaned = strip_tags(cue_line.trim());
                if !cleaned.is_empty() {
                    cue_lines.push(cleaned);
                }
            }
        }
        // YouTube auto-captions roll the previous bottom line to the top of the next cue.
        if cue_lines.len() > 1
            && let Some(prev) = cues.last()
            && start_ms <= prev.end_ms.saturating_add(500)
            && (cue_lines[0] == prev.text
                || last_cue_bottom_line.as_deref() == Some(cue_lines[0].as_str()))
        {
            cue_lines.remove(0);
        }
        let new_bottom_line = cue_lines.last().cloned();
        let text = cue_lines.join(" ");
        if !text.is_empty() {
            if let Some(prev) = cues.last_mut()
                && prev.text == text
                && start_ms <= prev.end_ms.saturating_add(250)
            {
                prev.end_ms = prev.end_ms.max(end_ms);
            } else {
                cues.push(Cue {
                    start_ms,
                    end_ms,
                    text,
                });
            }
            last_cue_bottom_line = new_bottom_line;
        }
        if cues.len() > 20_000 {
            return Err("legenda excede o limite de 20.000 trechos");
        }
    }
    Ok(cues)
}

/// Returns the cue active at a playback position. Cues are normally ordered.
pub fn active_cue_index(cues: &[Cue], position_ms: u64) -> Option<usize> {
    let end = cues.partition_point(|cue| cue.start_ms <= position_ms);
    (0..end).rev().find(|&index| {
        position_ms
            < cues[index]
                .end_ms
                .max(cues[index].start_ms.saturating_add(1))
    })
}

fn timestamp_ms(s: &str) -> Option<u64> {
    let fields: Vec<_> = s.split(':').collect();
    if !(2..=3).contains(&fields.len()) {
        return None;
    }
    let (hours, minutes, seconds) = if fields.len() == 3 {
        (
            fields[0].parse::<u64>().ok()?,
            fields[1].parse::<u64>().ok()?,
            fields[2],
        )
    } else {
        (0, fields[0].parse::<u64>().ok()?, fields[1])
    };
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, "0"));
    let sec = whole.parse::<u64>().ok()?;
    let millis = match fraction.len() {
        1 => fraction.parse::<u64>().ok()? * 100,
        2 => fraction.parse::<u64>().ok()? * 10,
        _ => fraction.get(..3)?.parse::<u64>().ok()?,
    };
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(sec.checked_mul(1000)?)?
        .checked_add(millis.min(999))
}

fn is_horizontal_whitespace(ch: char) -> bool {
    ch.is_whitespace()
        && !matches!(
            ch,
            '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
        )
}

fn decode_entity(entity: &str) -> Option<char> {
    let ch = match entity {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        _ => {
            let code = if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                u32::from_str_radix(hex, 16).ok()?
            } else {
                let dec = entity.strip_prefix('#')?;
                if dec.is_empty() || !dec.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                dec.parse::<u32>().ok()?
            };
            char::from_u32(code)?
        }
    };
    Some(if ch == '\u{00a0}' { ' ' } else { ch })
}

fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'#') {
                j += 1;
            }
            if j < bytes.len()
                && bytes[j] == b';'
                && j > i + 1
                && let Some(decoded) = decode_entity(&s[i + 1..j])
            {
                out.push(decoded);
                i = j + 1;
                continue;
            }
            out.push('&');
            i += 1;
        } else {
            let next_amp = s[i..].find('&').map_or(s.len(), |off| i + off);
            out.push_str(&s[i..next_amp]);
            i = next_amp;
        }
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut stripped = String::with_capacity(s.len());
    let mut inside = false;
    for ch in s.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => stripped.push(ch),
            _ => {}
        }
    }
    let decoded = decode_html_entities(&stripped);
    let trimmed = decoded.trim();
    let mut out = String::with_capacity(trimmed.len());
    let mut in_space = false;
    for ch in trimmed.chars() {
        if is_horizontal_whitespace(ch) {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out
}

pub fn format_timestamp_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

pub fn format_srt_timestamp_ms(ms: u64) -> String {
    let millis = ms % 1000;
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

pub fn format_txt(cues: &[Cue]) -> String {
    cues.iter()
        .map(|cue| {
            format!(
                "[{}] {}",
                format_timestamp_ms(cue.start_ms),
                cue.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_markdown(title: &str, cues: &[Cue]) -> String {
    let mut out = String::new();
    let trimmed_title = title.trim();
    if !trimmed_title.is_empty() {
        out.push_str("# ");
        out.push_str(trimmed_title);
        out.push_str("\n\n");
    }
    let body = cues
        .iter()
        .map(|cue| {
            format!(
                "- **[{}]** {}",
                format_timestamp_ms(cue.start_ms),
                cue.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    out.push_str(&body);
    out
}

pub fn format_srt(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(index, cue)| {
            format!(
                "{}\n{} --> {}\n{}",
                index + 1,
                format_srt_timestamp_ms(cue.start_ms),
                format_srt_timestamp_ms(cue.end_ms),
                cue.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn normalize_search_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\u{0300}'..='\u{036f}') {
            continue;
        }
        let folded = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => {
                'a'
            }
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => {
                'o'
            }
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            'ý' | 'ÿ' | 'Ý' | 'Ÿ' => 'y',
            _ => {
                for lower in ch.to_lowercase() {
                    out.push(lower);
                }
                continue;
            }
        };
        out.push(folded);
    }
    out
}

pub fn search<'a>(cues: &'a [Cue], query: &str) -> Vec<&'a Cue> {
    let q = normalize_search_text(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    cues.iter()
        .filter(|cue| normalize_search_text(&cue.text).contains(&q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_searches_cues() {
        let cues = parse_vtt(
            "WEBVTT\n\n00:01.200 --> 00:03.000\n<c>x &amp; y</c>\n\n00:00:04.000 --> 00:05.000\nBye\n",
        )
        .unwrap();
        assert_eq!(cues[0].start_ms, 1200);
        assert_eq!(search(&cues, "x & y").len(), 1);
    }

    #[test]
    fn rejects_oversized_and_ignores_bad_timestamps() {
        assert!(parse_vtt(&"x".repeat(2 * 1024 * 1024 + 1)).is_err());
        assert!(parse_vtt("00:XX --> 00:02\nno").unwrap().is_empty());
    }

    #[test]
    fn active_cue_tracks_boundaries_overlaps_and_gaps() {
        let cues = vec![
            Cue {
                start_ms: 0,
                end_ms: 1000,
                text: "first".into(),
            },
            Cue {
                start_ms: 500,
                end_ms: 1500,
                text: "overlap".into(),
            },
            Cue {
                start_ms: 2000,
                end_ms: 2500,
                text: "later".into(),
            },
        ];
        assert_eq!(active_cue_index(&cues, 0), Some(0));
        assert_eq!(active_cue_index(&cues, 500), Some(1));
        assert_eq!(active_cue_index(&cues, 1000), Some(1));
        assert_eq!(active_cue_index(&cues, 1500), None);
        assert_eq!(active_cue_index(&cues, 2000), Some(2));
        assert_eq!(active_cue_index(&cues, 2500), None);
    }

    #[test]
    fn formats_transcript_as_txt_and_markdown() {
        let cues = vec![
            Cue {
                start_ms: 5_000,
                end_ms: 10_000,
                text: "Introdução".into(),
            },
            Cue {
                start_ms: 3_665_000,
                end_ms: 3_670_000,
                text: "Conclusão".into(),
            },
        ];
        assert_eq!(format_txt(&cues), "[00:05] Introdução\n[1:01:05] Conclusão");
        assert_eq!(
            format_markdown("Meu Vídeo", &cues),
            "# Meu Vídeo\n\n- **[00:05]** Introdução\n- **[1:01:05]** Conclusão"
        );
    }

    #[test]
    fn strips_tags_decodes_entities_and_normalizes_whitespace() {
        let named = strip_tags(
            "<c>  &quot;Tom&quot; &amp; &apos;Jerry&#39; &lt;3&gt; &ndash; &mdash; &hellip;  </c>",
        );
        assert_eq!(named, "\"Tom\" & 'Jerry' <3> – — …");

        let numeric = strip_tags("&#65;&#x42;&#X43;&#x1F600;");
        assert_eq!(numeric, "ABC😀");

        let nbsp = strip_tags("  foo\t&nbsp;&#160;&#xA0;bar  ");
        assert_eq!(nbsp, "foo bar");
        assert_eq!(strip_tags("<c>&nbsp;</c>"), "");

        let untouched = strip_tags(
            "A & B &amp; &unknown; &#; &#x; &#abc; &#xD800; &#x110000; &#999999999999; &amp;lt;",
        );
        assert_eq!(
            untouched,
            "A & B & &unknown; &#; &#x; &#abc; &#xD800; &#x110000; &#999999999999; &lt;"
        );
    }

    #[test]
    fn skips_style_note_and_region_blocks_containing_arrows() {
        let vtt = "\u{feff}WEBVTT\n\n\
STYLE\n\
::cue {\n\
  /* 00:00.000 --> 00:01.000 */\n\
  color: white;\n\
}\n\n\
REGION\n\
id:top\n\
00:01.000 --> 00:02.000\n\n\
NOTE inline 00:02.000 --> 00:03.000\n\n\
NOTE\n\
00:03.000 --> 00:04.000\n\
Commented out cue\n\n\
00:05.000 --> 00:06.500\n\
Actual cue\n\n\
NOTE trailing comment 00:07.000 --> 00:08.000";

        let cues = parse_vtt(vtt).unwrap();
        assert_eq!(
            cues,
            vec![Cue {
                start_ms: 5_000,
                end_ms: 6_500,
                text: "Actual cue".into(),
            }]
        );
    }

    #[test]
    fn deduplicates_rollup_and_adjacent_duplicate_cues() {
        let vtt = "WEBVTT\n\n\
00:01.000 --> 00:02.000\n\
First line\n\
Second line\n\n\
00:02.100 --> 00:03.000\n\
<c>&nbsp;</c>\n\n\
00:02.200 --> 00:04.000\n\
Second line\n\
Third line\n\n\
00:04.200 --> 00:05.000\n\
Third line\n\n\
00:05.100 --> 00:06.000\n\
Third line\n\
Fourth line\n\n\
00:06.600 --> 00:08.000\n\
Fourth line\n\
Fifth line\n\n\
00:08.400 --> 00:09.000\n\
Fourth line Fifth line";

        let cues = parse_vtt(vtt).unwrap();
        assert_eq!(
            cues,
            vec![
                Cue {
                    start_ms: 1_000,
                    end_ms: 2_000,
                    text: "First line Second line".into(),
                },
                Cue {
                    start_ms: 2_200,
                    end_ms: 5_000,
                    text: "Third line".into(),
                },
                Cue {
                    start_ms: 5_100,
                    end_ms: 6_000,
                    text: "Fourth line".into(),
                },
                Cue {
                    start_ms: 6_600,
                    end_ms: 8_000,
                    text: "Fourth line Fifth line".into(),
                },
                Cue {
                    start_ms: 8_400,
                    end_ms: 9_000,
                    text: "Fourth line Fifth line".into(),
                },
            ]
        );
    }

    #[test]
    fn searches_case_and_accent_insensitively() {
        let cues = vec![
            Cue {
                start_ms: 0,
                end_ms: 1_000,
                text: "Programação em Rust".into(),
            },
            Cue {
                start_ms: 1_000,
                end_ms: 2_000,
                text: "Café, résumé, naïve, caña, garçon, Øresund, Über, ýŸ".into(),
            },
        ];
        assert_eq!(search(&cues, "programacao").len(), 1);
        assert_eq!(search(&cues, "PROGRAMAÇÃO").len(), 1);
        assert_eq!(
            search(
                &cues,
                "cafe, resume, naive, cana, garcon, oresund, uber, yy"
            )
            .len(),
            1
        );
        assert!(search(&cues, "   ").is_empty());
    }

    #[test]
    fn formats_srt_timestamps_and_subtitles() {
        assert_eq!(format_srt_timestamp_ms(0), "00:00:00,000");
        assert_eq!(format_srt_timestamp_ms(5_200), "00:00:05,200");
        assert_eq!(format_srt_timestamp_ms(3_665_045), "01:01:05,045");

        let cues = vec![
            Cue {
                start_ms: 5_200,
                end_ms: 8_000,
                text: "  Olá mundo  ".into(),
            },
            Cue {
                start_ms: 3_665_045,
                end_ms: 3_670_000,
                text: "Até logo".into(),
            },
        ];
        assert_eq!(format_srt(&[]), "");
        assert_eq!(
            format_srt(&cues),
            "1\n00:00:05,200 --> 00:00:08,000\nOlá mundo\n\n2\n01:01:05,045 --> 01:01:10,000\nAté logo"
        );
    }
}

//! Safe, bounded WebVTT parsing and transcript search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn parse_vtt(input: &str) -> Result<Vec<Cue>, &'static str> {
    if input.len() > 2 * 1024 * 1024 {
        return Err("legenda excede o limite de 2 MiB");
    }
    let mut cues = Vec::new();
    let mut lines = input.lines().peekable();
    while let Some(line) = lines.next() {
        let line = line.trim_start_matches('\u{feff}').trim();
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
        let mut text = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                lines.next();
                break;
            }
            if next.contains("-->") {
                break;
            }
            if let Some(cue_line) = lines.next() {
                text.push(cue_line.trim());
            }
        }
        let text = strip_tags(&text.join(" "));
        if !text.is_empty() {
            cues.push(Cue {
                start_ms,
                end_ms,
                text,
            });
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
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut inside = false;
    for ch in s.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
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

pub fn search<'a>(cues: &'a [Cue], query: &str) -> Vec<&'a Cue> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    cues.iter()
        .filter(|cue| cue.text.to_lowercase().contains(&q))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_searches_cues() {
        let cues=parse_vtt("WEBVTT\n\n00:01.200 --> 00:03.000\n<c>x &amp; y</c>\n\n00:00:04.000 --> 00:05.000\nBye\n").unwrap();
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
}

//! 1-based line range specifications such as `{1,3-5}` for code fences.

use std::ops::RangeInclusive;

/// A sorted set of 1-based inclusive line ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineRangeSet {
    ranges: Vec<RangeInclusive<usize>>,
}

impl LineRangeSet {
    /// Whether `line` (1-based) is in the highlighted set.
    pub fn contains_one_based(&self, line: usize) -> bool {
        self.ranges.iter().any(|range| range.contains(&line))
    }

    /// Whether `line_index` (0-based) is in the highlighted set.
    pub fn contains_zero_based(&self, line_index: usize) -> bool {
        self.contains_one_based(line_index + 1)
    }

    /// Whether the set has no highlighted lines.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Formats the set in canonical `1,3-5` notation.
    pub fn to_spec(&self) -> String {
        self.ranges
            .iter()
            .map(|range| {
                let (start, end) = (*range.start(), *range.end());
                if start == end {
                    start.to_string()
                } else {
                    format!("{start}-{end}")
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Parses a line range expression such as `"{1, 3-5}"` or `"2-4,8"`.
pub fn parse_line_ranges(spec: &str) -> LineRangeSet {
    let mut raw_ranges: Vec<(usize, usize)> = Vec::new();
    for part in spec.trim().trim_matches(['{', '}']).split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let parsed = match part.split_once('-') {
            Some((start, end)) => start
                .trim()
                .parse::<usize>()
                .ok()
                .zip(end.trim().parse::<usize>().ok()),
            None => part.parse::<usize>().ok().map(|line| (line, line)),
        };
        if let Some((start, end)) = parsed
            && start >= 1
            && start <= end
            && end <= 1_000_000
        {
            raw_ranges.push((start, end));
        }
    }
    raw_ranges.sort_unstable();
    let mut merged: Vec<RangeInclusive<usize>> = Vec::new();
    for (start, end) in raw_ranges {
        match merged.last_mut() {
            Some(prev) if start <= prev.end().saturating_add(1) => {
                let next_end = (*prev.end()).max(end);
                *prev = *prev.start()..=next_end;
            }
            _ => merged.push(start..=end),
        }
    }
    LineRangeSet { ranges: merged }
}

//! How a text differs from the one it was, a word at a time, for a view
//! to strike out what went and mark what came.

use std::ops::Range;

/// A stretch of a changed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diff {
    /// In both texts: where it is in the new one.
    Kept(Range<usize>),
    /// Only in the new text.
    Added(Range<usize>),
    /// Only in the old text: where it was there.
    Removed(Range<usize>),
}

/// The words of `text` and each character between them, as byte ranges.
fn words(text: &str) -> Vec<Range<usize>> {
    let mut words: Vec<Range<usize>> = Vec::new();
    let mut in_word = false;
    for (at, character) in text.char_indices() {
        let end = at + character.len_utf8();
        let letter = character.is_alphanumeric();
        match words.last_mut() {
            Some(word) if letter && in_word => word.end = end,
            _ => words.push(at..end),
        }
        in_word = letter;
    }
    words
}

/// How `new` differs from `old`, in the order of `new`: what a change
/// removed comes before what it added, each as one stretch.
pub fn word_diff(old: &str, new: &str) -> Vec<Diff> {
    let (was, is) = (words(old), words(new));
    // shortcut: very long texts are one replacement; a diff in linear
    // space when reviews of such blocks show.
    if was.len() * is.len() > 250_000 {
        return vec![Diff::Removed(0..old.len()), Diff::Added(0..new.len())];
    }
    let same = |a: usize, b: usize| old[was[a].clone()] == new[is[b].clone()];
    // How many words the rests of both from each pair of places share.
    let width = is.len() + 1;
    let mut shared = vec![0u32; (was.len() + 1) * width];
    for a in (0..was.len()).rev() {
        for b in (0..is.len()).rev() {
            shared[a * width + b] = match same(a, b) {
                true => shared[(a + 1) * width + b + 1] + 1,
                false => shared[(a + 1) * width + b].max(shared[a * width + b + 1]),
            };
        }
    }
    let mut diff = Vec::new();
    let (mut removed, mut added): (Option<Range<usize>>, Option<Range<usize>>) = (None, None);
    let grow = |range: &mut Option<Range<usize>>, word: &Range<usize>| match range {
        Some(range) => range.end = word.end,
        None => *range = Some(word.clone()),
    };
    let (mut a, mut b) = (0, 0);
    while a < was.len() || b < is.len() {
        if a < was.len() && b < is.len() && same(a, b) {
            diff.extend(removed.take().map(Diff::Removed));
            diff.extend(added.take().map(Diff::Added));
            match diff.last_mut() {
                Some(Diff::Kept(kept)) => kept.end = is[b].end,
                _ => diff.push(Diff::Kept(is[b].clone())),
            }
            (a, b) = (a + 1, b + 1);
        } else if b == is.len()
            || a < was.len() && shared[(a + 1) * width + b] >= shared[a * width + b + 1]
        {
            grow(&mut removed, &was[a]);
            a += 1;
        } else {
            grow(&mut added, &is[b]);
            b += 1;
        }
    }
    diff.extend(removed.map(Diff::Removed));
    diff.extend(added.map(Diff::Added));
    diff
}

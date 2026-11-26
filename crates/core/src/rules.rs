//! Input rules: what typed text turns into. The patterns are those of
//! Markdown, as Tiptap and BlockNote apply them.

use std::ops::Range;

use crate::{MAX_HEADING_LEVEL, Value, attr, block, mark};

/// What a paragraph becomes.
#[derive(Debug, PartialEq)]
pub(crate) struct BlockRule {
    pub(crate) block_type: &'static str,
    pub(crate) attrs: Vec<(&'static str, Value)>,
}

fn block(block_type: &'static str) -> Option<BlockRule> {
    let attrs = Vec::new();
    Some(BlockRule { block_type, attrs })
}

fn block_with(block_type: &'static str, key: &'static str, value: impl Into<Value>) -> BlockRule {
    let attrs = vec![(key, value.into())];
    BlockRule { block_type, attrs }
}

/// The block that a paragraph starting with `typed`, up to the caret, turns into.
pub(crate) fn block_rule(typed: &str) -> Option<BlockRule> {
    if typed == "---" {
        return block(block::DIVIDER);
    }
    let marker = typed.strip_suffix(' ')?;
    match marker {
        "-" | "+" | "*" => return block(block::BULLET_LIST_ITEM),
        ">" => return block(block::QUOTE),
        "[]" | "[ ]" => return Some(block_with(block::CHECK_LIST_ITEM, "checked", false)),
        "[x]" | "[X]" => return Some(block_with(block::CHECK_LIST_ITEM, "checked", true)),
        _ => {}
    }
    let level = marker.len() as i64;
    if (1..=MAX_HEADING_LEVEL).contains(&level) && marker.bytes().all(|b| b == b'#') {
        return Some(block_with(block::HEADING, "level", level));
    }
    if let Some(info) = marker.strip_prefix("```") {
        let plain = !info.contains(|c: char| c == '`' || c.is_whitespace());
        return plain.then(|| {
            let mut attrs = Vec::new();
            let (language, title) = match info.split_once(':') {
                Some((lang, title)) => (lang, (!title.is_empty()).then_some(title)),
                None => (info, None),
            };
            if !language.is_empty() {
                attrs.push((attr::LANGUAGE, language.into()));
            }
            if let Some(title) = title {
                attrs.push((attr::TITLE, title.into()));
            }
            BlockRule {
                block_type: block::CODE_BLOCK,
                attrs,
            }
        });
    }
    let number = marker.strip_suffix('.')?;
    // More digits than a list has items would not fit the attribute either.
    if (1..=9).contains(&number.len()) && number.bytes().all(|b| b.is_ascii_digit()) {
        let start: i64 = number.parse().ok()?;
        let mut rule = block(block::NUMBERED_LIST_ITEM)?;
        if start != 1 {
            rule.attrs.push((attr::START, start.into()));
        }
        return Some(rule);
    }
    None
}

/// A mark to put on the text between two delimiters, which go.
#[derive(Debug, PartialEq)]
pub(crate) struct MarkRule {
    pub(crate) mark: &'static str,
    pub(crate) open: Range<usize>,
    pub(crate) close: Range<usize>,
}

/// The mark that the end of `typed`, the text of a block up to the caret, asks for.
pub(crate) fn mark_rule(typed: &str) -> Option<MarkRule> {
    // Longer delimiters first: `**` ends in `*`.
    let rules = [
        ("**", mark::BOLD),
        ("~~", mark::STRIKE),
        ("`", mark::CODE),
        ("*", mark::ITALIC),
        ("_", mark::ITALIC),
    ];
    rules.into_iter().find_map(|(delimiter, mark)| {
        let before = typed.strip_suffix(delimiter)?;
        let open = before.rfind(delimiter)?;
        let content = &before[open + delimiter.len()..];
        let last = before[..open].chars().next_back();
        // Code may follow anything but another backtick. The others start a
        // word, so that `2*3*4` and `snake_case_` stay as typed.
        let starts = match delimiter {
            "`" => last != Some('`'),
            _ => last.is_none_or(char::is_whitespace),
        };
        (starts && !content.trim().is_empty()).then(|| MarkRule {
            mark,
            open: open..open + delimiter.len(),
            close: before.len()..typed.len(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(typed: &str) -> Option<(&'static str, Vec<(&'static str, Value)>)> {
        block_rule(typed).map(|rule| (rule.block_type, rule.attrs))
    }

    #[test]
    fn block_rules_match_markdown_prefixes() {
        assert_eq!(
            attrs("# "),
            Some((block::HEADING, vec![("level", 1.into())]))
        );
        assert_eq!(
            attrs("##### "),
            Some((block::HEADING, vec![("level", 5.into())]))
        );
        assert_eq!(attrs("###### "), None);
        for bullet in ["- ", "+ ", "* "] {
            assert_eq!(attrs(bullet), Some((block::BULLET_LIST_ITEM, vec![])));
        }
        assert_eq!(attrs("1. "), Some((block::NUMBERED_LIST_ITEM, vec![])));
        assert_eq!(
            attrs("12. "),
            Some((block::NUMBERED_LIST_ITEM, vec![("start", 12.into())]))
        );
        assert_eq!(attrs("1234567890. "), None);
        assert_eq!(
            attrs("[] "),
            Some((block::CHECK_LIST_ITEM, vec![("checked", false.into())]))
        );
        assert_eq!(
            attrs("[x] "),
            Some((block::CHECK_LIST_ITEM, vec![("checked", true.into())]))
        );
        assert_eq!(attrs("> "), Some((block::QUOTE, vec![])));
        assert_eq!(attrs("``` "), Some((block::CODE_BLOCK, vec![])));
        assert_eq!(
            attrs("```rust "),
            Some((block::CODE_BLOCK, vec![("language", "rust".into())]))
        );
        assert_eq!(attrs("---"), Some((block::DIVIDER, vec![])));
        for plain in ["#", "#x ", "a - ", "-", "1 ", ". ", "--- ", "```a b "] {
            assert_eq!(attrs(plain), None, "{plain:?}");
        }
    }

    #[test]
    fn mark_rules_take_the_nearest_opening_delimiter() {
        let rule = |typed| mark_rule(typed).map(|rule| (rule.mark, rule.open, rule.close));
        assert_eq!(rule("a **b c**"), Some(("bold", 2..4, 7..9)));
        assert_eq!(rule("*é*"), Some(("italic", 0..1, 3..4)));
        assert_eq!(rule("_x_"), Some(("italic", 0..1, 2..3)));
        assert_eq!(rule("~~x~~"), Some(("strike", 0..2, 3..5)));
        assert_eq!(rule("f(`x`"), Some(("code", 2..3, 4..5)));
        assert_eq!(rule("`a` `b`"), Some(("code", 4..5, 6..7)));
        for plain in [
            "**bold*",
            "****",
            "** **",
            "2*3*",
            "snake_case_",
            "``x`",
            "*x",
            "x*",
        ] {
            assert_eq!(rule(plain), None, "{plain:?}");
        }
    }
}

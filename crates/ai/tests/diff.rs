use lotype_ai::{Diff, word_diff};

/// The diff as text: `[-gone-]`, `{+come+}`, and the rest as it is.
fn marked(old: &str, new: &str) -> String {
    let mut out = String::new();
    for part in word_diff(old, new) {
        match part {
            Diff::Kept(range) => out.push_str(&new[range]),
            Diff::Added(range) => out.push_str(&format!("{{+{}+}}", &new[range])),
            Diff::Removed(range) => out.push_str(&format!("[-{}-]", &old[range])),
        }
    }
    out
}

#[test]
fn a_changed_word_is_struck_before_what_took_its_place() {
    assert_eq!(
        marked("in the pilot chanel.", "in the pilot channel."),
        "in the pilot [-chanel-]{+channel+}."
    );
    assert_eq!(
        marked("opens on Thursday.", "opens on Friday, 14 March."),
        "opens on [-Thursday-]{+Friday, 14 March+}."
    );
}

#[test]
fn text_only_added_or_only_removed_is_one_stretch() {
    assert_eq!(marked("one two", "one two three"), "one two{+ three+}");
    assert_eq!(marked("one two three", "one three"), "one [-two -]three");
    assert_eq!(marked("", "new"), "{+new+}");
    assert_eq!(marked("old", ""), "[-old-]");
    assert_eq!(marked("same text", "same text"), "same text");
}

#[test]
fn words_of_any_script_are_cut_between_characters() {
    assert_eq!(marked("naïve café", "naïve thé"), "naïve [-café-]{+thé+}");
    assert_eq!(marked("日本語", "日本"), "[-日本語-]{+日本+}");
}

use std::collections::BTreeMap;

use lotype_core::test_support::{build, outline};
use lotype_core::{
    BlockSpec, Content, Document, Expand, MAX_DEPTH, MarkSpec, Schema, Span, Value, b, blocks,
    bullet, h1, i, p,
};
use proptest::prelude::*;

fn read(markdown: &str) -> Document {
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, markdown).unwrap();
    doc
}

fn shown(doc: &Document) -> String {
    outline(doc, None)
}

const SAMPLE: &str = "\
# Title with *emphasis*

A paragraph with **bold**, *italic*, ~~strike~~, `code` and a [link](https://lotype.dev).
Second line of it.\\
After a hard break.

- one
- two
  - nested
  - [x] done
  - [ ] open

3. third
4. fourth

   More in the fourth item.

> A quote.
>
> With more in it.

> [!WARNING]
> Careful.

```rust
fn main() {}
```

---

<!-- page-break -->

![A caption](https://lotype.dev/a.png)
";

#[test]
fn markdown_is_read_into_blocks() {
    let expected = "\
heading level=1 \"Title with [italic|emphasis]\"
paragraph \"A paragraph with [bold|bold], [italic|italic], [strike|strike], [code|code] and a [link=https://lotype.dev|link]. Second line of it.\nAfter a hard break.\"
bullet_list_item \"one\"
bullet_list_item \"two\"
  bullet_list_item \"nested\"
  check_list_item checked=true \"done\"
  check_list_item checked=false \"open\"
numbered_list_item start=3 \"third\"
numbered_list_item \"fourth\"
  paragraph \"More in the fourth item.\"
quote \"A quote.\"
  paragraph \"With more in it.\"
alert kind=warning
  paragraph \"Careful.\"
code_block language=rust \"fn main() {}\"
divider
page_break
image caption=A caption url=https://lotype.dev/a.png
";
    assert_eq!(shown(&read(SAMPLE)), expected);
}

#[test]
fn blocks_are_written_as_markdown_that_reads_back_the_same() {
    let doc = read(SAMPLE);
    let markdown = doc.to_markdown();
    let expected = "\
# Title with *emphasis*

A paragraph with **bold**, *italic*, ~~strike~~, `code` and a [link](https://lotype.dev). Second line of it.\\
After a hard break.

- one

- two

  - nested

  - [x] done

  - [ ] open

3. third

4. fourth

   More in the fourth item.

> A quote.
>
> With more in it.

> [!WARNING]
>
> Careful.

```rust
fn main() {}
```

---

<!-- page-break -->

![A caption](https://lotype.dev/a.png)
";
    assert_eq!(markdown, expected);
    assert_eq!(shown(&read(&markdown)), shown(&doc));
    assert_eq!(read(&markdown).to_markdown(), markdown);
}

#[test]
fn what_markdown_cannot_say_is_written_as_plainly_as_it_can_be() {
    let (mut doc, _) = build(blocks![
        p!["top", p!["nested in a paragraph"]],
        h1![b!["bold ", i!["and italic"]], " title"],
        bullet!["item", p!["under it"]]
    ]);
    let top = doc.blocks()[0];
    doc.mark(top, 0..3, "underline", true).unwrap();
    doc.mark(top, 0..3, "text_color", "red").unwrap();
    assert_eq!(
        doc.to_markdown(),
        "top\n\nnested in a paragraph\n\n# **bold *and italic*** title\n\n- item\n\n  under it\n"
    );
}

#[test]
fn text_that_looks_like_markup_stays_text() {
    for text in [
        "a*b*c _d_ [x](y) <tag> & `tick` $5 ~~no~~ \\ back",
        "# not a heading",
        "- not a list",
        "+ not a list",
        "1. not a list",
        "12) not a list",
        "> not a quote",
        "---",
        "=== not a heading rule",
        "    not code",
        "line one\n- line two\n# line three\n  indented four",
        "![not an image](x)",
        "&amp; stays",
    ] {
        let (doc, _) = build(blocks![p!["before"], p![text], p!["after"]]);
        let markdown = doc.to_markdown();
        assert_eq!(
            shown(&read(&markdown)),
            shown(&doc),
            "{text:?} as {markdown:?}"
        );
    }
}

#[test]
fn code_and_addresses_survive_what_would_end_them() {
    let (mut doc, _) = build(blocks![p!["a `tick` b and c"]]);
    let block = doc.blocks()[0];
    doc.mark(block, 2..8, "code", true).unwrap();
    doc.mark(block, 15..16, "link", "https://x.y/a (b) <c>")
        .unwrap();
    let code = doc.insert_block(None, 1, "code_block").unwrap();
    doc.insert_text(code, 0, "```\nfence inside\n```").unwrap();
    let markdown = doc.to_markdown();
    let back = read(&markdown);
    assert_eq!(back.text(back.blocks()[0]).unwrap(), "a `tick` b and c");
    assert_eq!(
        back.text(back.blocks()[1]).unwrap(),
        "```\nfence inside\n```"
    );
    assert!(shown(&back).contains("[link=https://x.y/a (b) %3Cc%3E|c]"));
}

#[test]
fn nesting_beyond_what_a_document_holds_is_flattened() {
    let deep = "> ".repeat(MAX_DEPTH * 3) + "bottom";
    let doc = read(&deep);
    assert!(doc.blocks().len() >= MAX_DEPTH);
    let last = *doc.blocks().last().unwrap();
    assert_eq!(doc.text(last).unwrap(), "bottom");

    let list: String = (0..MAX_DEPTH * 2)
        .map(|level| format!("{}- {level}\n", "  ".repeat(level)))
        .collect();
    assert_eq!(read(&list).blocks().len(), MAX_DEPTH * 2);
}

#[test]
fn a_schema_without_a_block_type_or_a_mark_keeps_the_text() {
    let schema = Schema::new().block(BlockSpec::new("paragraph", Content::Inline));
    let mut doc = Document::new(schema);
    let made = doc
        .insert_markdown(None, 0, "# **Title**\n\n- item\n\n---\n")
        .unwrap();
    assert_eq!(made.len(), 2);
    assert_eq!(shown(&doc), "paragraph \"Title\"\nparagraph \"item\"\n");
}

#[test]
fn markdown_goes_where_it_is_asked_to() {
    let (mut doc, _) = build(blocks![bullet!["a"], bullet!["b"]]);
    let a = doc.blocks()[0];
    let made = doc.insert_markdown(Some(a), 0, "one\n\ntwo").unwrap();
    assert_eq!(made, doc.children(Some(a)).unwrap());
    let made = doc.insert_markdown(None, 1, "- x\n  - y").unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(
        shown(&doc),
        "bullet_list_item \"a\"\n  paragraph \"one\"\n  paragraph \"two\"\nbullet_list_item \"x\"\n  bullet_list_item \"y\"\nbullet_list_item \"b\"\n"
    );
    assert!(doc.insert_markdown(None, 9, "x").is_err());
}

/// One paragraph, or a block of another type, with these spans.
fn rich(block_type: &str, spans: &[(&str, &[&str])]) -> Document {
    let mut doc = Document::new(Schema::builtin());
    let block = doc.insert_block(None, 0, block_type).unwrap();
    let spans: Vec<_> = spans
        .iter()
        .map(|(text, marks)| {
            let value = |mark: &&str| match *mark {
                "link" => Value::from("u"),
                _ => Value::from(true),
            };
            let marks: BTreeMap<_, _> = marks.iter().map(|m| ((*m).to_owned(), value(m))).collect();
            Span {
                text: (*text).to_owned(),
                marks,
            }
        })
        .collect();
    doc.insert_spans(block, 0, &spans).unwrap();
    doc
}

/// The characters of a document that are no space, each with its marks.
fn marked(doc: &Document) -> Vec<(char, Vec<String>)> {
    let mut out = Vec::new();
    for block in doc.blocks() {
        for span in doc.spans(block).unwrap_or_default() {
            let marks: Vec<_> = span.marks.keys().cloned().collect();
            let chars = span.text.chars().filter(|c| !c.is_whitespace());
            out.extend(chars.map(|c| (c, marks.clone())));
        }
    }
    out
}

fn round_trips(doc: &Document) {
    let markdown = doc.to_markdown();
    assert_eq!(shown(&read(&markdown)), shown(doc), "as {markdown:?}");
}

#[test]
fn marks_that_touch_text_or_each_other_read_back() {
    let cases: &[&[(&str, &[&str])]] = &[
        &[("a.", &["bold"]), ("b", &[])],
        &[("x", &[]), ("(y)", &["bold"])],
        &[("这是", &[]), ("重要的。", &["bold"]), ("下一句", &[])],
        &[("a", &[]), ("a", &["bold", "strike"])],
        &[("a", &["bold", "code", "link"]), ("a", &["link"])],
        &[
            ("a", &["bold"]),
            ("b", &["bold", "italic"]),
            ("c", &["italic"]),
        ],
        &[
            ("a", &["italic"]),
            ("a", &["bold"]),
            ("a", &["bold", "italic"]),
        ],
        &[("*", &["bold"]), ("x", &[])],
        &[("a", &[]), (" ", &["code"]), ("b", &[])],
        &[
            ("a", &["italic", "link"]),
            (" ", &["link"]),
            ("b", &["bold", "link"]),
        ],
    ];
    for spans in cases {
        let doc = rich("paragraph", spans);
        let back = read(&doc.to_markdown());
        assert_eq!(
            marked(&back),
            marked(&doc),
            "{spans:?} as {:?}",
            doc.to_markdown()
        );
    }
    // One link, not two with a gap.
    let doc = read("[*a* **b**](u)");
    assert_eq!(doc.to_markdown(), "[*a* **b**](u)\n");
}

#[test]
fn line_breaks_and_tabs_stay_what_they_are() {
    // The break stays a break. Which side of it the bold ends on is not kept.
    let doc = rich("paragraph", &[("a\n", &["bold"]), ("b", &[])]);
    let back = read(&doc.to_markdown());
    assert_eq!(back.text(back.blocks()[0]).unwrap(), "a\nb");
    assert_eq!(marked(&back), marked(&doc));
    for text in ["a\r# b", "\tfoo", "a\n\t- b", "one\r\n# two\r- three"] {
        round_trips(&rich("paragraph", &[(text, &[])]));
        round_trips(&rich("bullet_list_item", &[(text, &[])]));
    }
    // A heading is one line.
    let heading = rich("heading", &[("a\n", &["bold"]), ("b\rc #", &[])]);
    let back = read(&heading.to_markdown());
    assert_eq!(back.blocks().len(), 1);
    assert_eq!(back.text(back.blocks()[0]).unwrap(), "a b\rc #");
    for title in ["a #", "a ##", "#"] {
        let back = read(&rich("heading", &[(title, &[])]).to_markdown());
        assert_eq!(back.text(back.blocks()[0]).unwrap(), title);
    }
}

#[test]
fn list_items_keep_what_is_in_them() {
    for markdown in [
        "- [ ] a\n  - [ ] b\n    - c",
        "- > q",
        "- - a",
        "1. - a\n   - b",
        "- ![alt](u)",
        "- # h\n\n  text",
    ] {
        let doc = read(markdown);
        round_trips(&doc);
        assert_eq!(doc.children(None).unwrap().len(), 1, "{markdown:?}");
    }
}

#[test]
fn numbers_no_list_can_start_at_are_brought_in_range() {
    for start in [i64::MAX, 1_000_000_000, -5] {
        let mut doc = Document::new(Schema::builtin());
        for index in 0..2 {
            let item = doc.insert_block(None, index, "numbered_list_item").unwrap();
            doc.insert_text(item, 0, "x").unwrap();
        }
        doc.set_attr(doc.blocks()[0], "start", start).unwrap();
        let back = read(&doc.to_markdown());
        let types: Vec<_> = back
            .blocks()
            .iter()
            .map(|b| back.block_type(*b).unwrap())
            .collect();
        assert_eq!(
            types,
            ["numbered_list_item", "numbered_list_item"],
            "{start}"
        );
    }
}

#[test]
fn images_and_addresses_keep_what_they_point_to() {
    let doc = read("a ![](http://u/i.png) b and ![](u1) ![](u2)");
    assert_eq!(
        shown(&doc),
        "paragraph \"a [link=http://u/i.png|http://u/i.png] b and [link=u1|u1] [link=u2|u2]\"\n"
    );
    for url in [
        "\\\\server\\share\\f.pdf",
        "http://x/a\\",
        "x y\\",
        "a\\*b",
        "?a=1&amp;b=2",
    ] {
        let mut doc = Document::new(Schema::builtin());
        let block = doc.insert_block(None, 0, "paragraph").unwrap();
        doc.insert_text(block, 0, "t").unwrap();
        doc.mark(block, 0..1, "link", url).unwrap();
        round_trips(&doc);
    }
    let mut doc = Document::new(Schema::builtin());
    let image = doc.insert_block(None, 0, "image").unwrap();
    doc.set_attr(image, "url", "u").unwrap();
    doc.set_attr(image, "caption", "a\n- b").unwrap();
    let back = read(&doc.to_markdown());
    assert_eq!(shown(&back), "image caption=a - b url=u\n");
}

#[test]
fn a_schema_that_differs_still_takes_everything_markdown_says() {
    let valued = Schema::builtin().mark(MarkSpec::new("code", Expand::None).with_value());
    let mut doc = Document::new(valued);
    doc.insert_markdown(None, 0, "before\n\na `b` c\n\nafter")
        .unwrap();
    assert_eq!(
        shown(&doc),
        "paragraph \"before\"\nparagraph \"a b c\"\nparagraph \"after\"\n"
    );

    let plain = Schema::new().block(BlockSpec::new("paragraph", Content::Inline));
    let mut doc = Document::new(plain);
    let markdown = "![A caption](https://x/a.png)\n\n> [!NOTE]\n> inside\n\n- ```\n  code\n  ```";
    doc.insert_markdown(None, 0, markdown).unwrap();
    assert_eq!(
        shown(&doc),
        "paragraph \"A caption\"\nparagraph \"inside\"\nparagraph \"code\"\n"
    );

    let textless = Schema::builtin().block(BlockSpec::new("quote", Content::None).with_children());
    let mut doc = Document::new(textless);
    doc.insert_markdown(None, 0, "> quoted words\n>\n> more")
        .unwrap();
    assert_eq!(
        shown(&doc),
        "quote\n  paragraph \"quoted words\"\n  paragraph \"more\"\n"
    );
}

#[test]
fn a_block_of_a_type_unknown_here_is_written_with_its_text() {
    let theirs =
        Schema::builtin().block(BlockSpec::new("callout", Content::Inline).with_children());
    let mut source = Document::new(theirs);
    let callout = source.insert_block(None, 0, "callout").unwrap();
    source.insert_text(callout, 0, "important *text*").unwrap();
    let child = source.insert_block(Some(callout), 0, "paragraph").unwrap();
    source.insert_text(child, 0, "child").unwrap();
    let mut doc = Document::new(Schema::builtin());
    doc.import(&source.export_snapshot().unwrap()).unwrap();
    assert_eq!(doc.to_markdown(), "important \\*text\\*\n\nchild\n");
}

#[test]
fn emphasis_nested_as_deep_as_the_text_is_long_reads_in_good_time() {
    let depth = 20_000;
    let doc = read(&("*a ".repeat(depth) + &"a* ".repeat(depth)));
    assert_eq!(doc.blocks().len(), 1);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Whatever a paragraph says, it says again after a trip through Markdown.
    #[test]
    fn any_text_reads_back(text in "[ a1é*_`\\[\\]()<>&#~$\\\\!.+=|:/-]{0,24}(\n[ a1*_#>+=.)-]{0,8}){0,2}") {
        // Space at the very ends, and before a line break, is not kept.
        let text = text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");
        let text = text.trim();
        prop_assume!(!text.is_empty());
        // Not through the builder, which reads `<a>` as a selection marker.
        let mut doc = Document::new(Schema::builtin());
        let block = doc.insert_block(None, 0, "paragraph").unwrap();
        doc.insert_text(block, 0, text).unwrap();
        let markdown = doc.to_markdown();
        let back = read(&markdown);
        prop_assert_eq!(back.blocks().len(), 1, "{:?}", markdown);
        prop_assert_eq!(back.text(back.blocks()[0]).unwrap(), text, "{:?}", markdown);
        prop_assert_eq!(shown(&back), shown(&doc), "{:?}", markdown);
    }

    /// Marks on spans that touch read back on the same characters.
    #[test]
    fn any_marks_read_back(
        spans in prop::collection::vec(("[ab.( )1é*_]{1,3}", prop::collection::btree_set(0usize..5, 0..3)), 1..5)
    ) {
        let names = ["bold", "italic", "strike", "code", "link"];
        let spans: Vec<(&str, Vec<&str>)> = spans
            .iter()
            .map(|(text, marks)| (text.as_str(), marks.iter().map(|m| names[*m]).collect()))
            .collect();
        let spans: Vec<(&str, &[&str])> = spans.iter().map(|(t, m)| (*t, m.as_slice())).collect();
        let doc = rich("paragraph", &spans);
        prop_assume!(!doc.text(doc.blocks()[0]).unwrap().trim().is_empty());
        let markdown = doc.to_markdown();
        let back = read(&markdown);
        prop_assert_eq!(marked(&back), marked(&doc), "{:?}", markdown);
    }
}

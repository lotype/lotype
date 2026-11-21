use lotype_core::test_support::{build, outline};
use lotype_core::{BlockSpec, Content, Document, MAX_DEPTH, Schema, b, blocks, bullet, h1, i, p};
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
}

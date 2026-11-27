use lotype_code::{
    CodeAnalysis, CodeBlockMeta, CutKind, DiagnosticAnnotation, HoverAnnotation, LineTag, Severity,
    canonical_language, clean_copy_text, language_label, parse_code_annotations, parse_line_ranges,
};
use lotype_core::{Document, Schema};

#[test]
fn stacked_queries_diagnostics_hovers_and_line_tags_map_to_exact_byte_ranges() {
    let code = "\
let setup = true;
// ---cut---
let (host, port) = (\"127.0.0.1\", 8080); // [!code highlight]
//   ^? &str
//         ^?
let broken: u16 = \"bad\"; // [!code error]
//                ^^^^^ @error(E0308): mismatched types: expected `u16`, found `&str`
// @hover host: let host: &str - Loopback bind address
let added = 1; // [!code ++]";

    let mut parsed = parse_code_annotations(code, Some("{1}"), true);
    assert_eq!(parsed.cuts, vec![(1, CutKind::Before)]);
    assert!(parsed.is_cut_line(0));
    assert!(parsed.is_cut_line(1));
    assert!(!parsed.is_cut_line(2));
    assert_eq!(parsed.queries.len(), 2);
    assert_eq!(parsed.queries[0].symbol, "host");
    assert_eq!(parsed.queries[0].target_line, 2);
    assert_eq!(parsed.queries[0].text.as_deref(), Some("&str"));
    assert_eq!(&code[parsed.queries[0].target_range.clone()], "host");

    assert_eq!(parsed.queries[1].symbol, "port");
    assert_eq!(parsed.queries[1].target_line, 2);
    assert_eq!(parsed.queries[1].text, None);
    assert_eq!(&code[parsed.queries[1].target_range.clone()], "port");

    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(parsed.diagnostics[0].severity, Severity::Error);
    assert_eq!(parsed.diagnostics[0].code.as_deref(), Some("E0308"));
    assert_eq!(&code[parsed.diagnostics[0].target_range.clone()], "\"bad\"");

    // Hover from `@hover` overrides with documentation, while `port` is filled by CodeAnalysis.
    let port_range = parsed.queries[1].target_range.clone();
    parsed.merge_analysis(&CodeAnalysis {
        queries: vec![(port_range.clone(), "u16".into())],
        completions: Vec::new(),
        hovers: vec![HoverAnnotation {
            range: port_range.clone(),
            symbol: "port".into(),
            signature: "let port: u16".into(),
            docs: Some("TCP port".into()),
        }],
        diagnostics: Vec::new(),
    });
    assert_eq!(parsed.queries[1].text.as_deref(), Some("u16"));

    let host_hover = parsed
        .hover_at(parsed.queries[0].target_range.start)
        .expect("host hover");
    assert_eq!(host_hover.signature, "let host: &str");
    assert_eq!(host_hover.docs.as_deref(), Some("Loopback bind address"));

    let port_hover = parsed.hover_at(port_range.start).expect("port hover");
    assert_eq!(port_hover.signature, "let port: u16");
    assert_eq!(port_hover.docs.as_deref(), Some("TCP port"));

    assert!(
        parsed
            .lines
            .iter()
            .any(|m| m.line == 0 && m.tag == LineTag::Highlight)
    );
    assert!(
        parsed
            .lines
            .iter()
            .any(|m| m.line == 2 && m.tag == LineTag::Highlight)
    );
    assert!(
        parsed
            .lines
            .iter()
            .any(|m| m.line == 5 && m.tag == LineTag::Error)
    );
    assert!(
        parsed
            .lines
            .iter()
            .any(|m| m.line == 8 && m.tag == LineTag::Added)
    );

    let copied = clean_copy_text(code, true);
    assert_eq!(
        copied,
        "let (host, port) = (\"127.0.0.1\", 8080);\nlet broken: u16 = \"bad\";\nlet added = 1;"
    );

    let extra_diag = DiagnosticAnnotation {
        target_range: port_range,
        after_line: 2,
        severity: Severity::Warning,
        code: None,
        message: "unused port".into(),
    };
    parsed.merge_analysis(&CodeAnalysis {
        diagnostics: vec![extra_diag],
        ..Default::default()
    });
    assert_eq!(parsed.diagnostics.len(), 2);
}

#[test]
fn line_ranges_and_language_aliases_normalize_cleanly() {
    let set = parse_line_ranges("{5, 2-3, 4, 9-10}");
    assert_eq!(set.to_spec(), "2-5,9-10");
    assert!(!set.contains_one_based(1));
    assert!(set.contains_one_based(3));
    assert!(set.contains_zero_based(4));

    assert_eq!(canonical_language("RS"), "rust");
    assert_eq!(canonical_language("ts"), "typescript");
    assert_eq!(language_label(Some("rs")), "Rust");
    assert_eq!(language_label(None), "Plain text");

    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(
        None,
        0,
        "```rs twoslash title=\"src/demo.rs\" {1}\nlet x = 1;\n//  ^? i32\n```\n",
    )
    .unwrap();
    let meta = CodeBlockMeta::of(&doc, doc.blocks()[0]).expect("code block meta");
    assert_eq!(meta.label, "Rust");
    assert_eq!(meta.title.as_deref(), Some("src/demo.rs"));
    assert!(meta.twoslash);
    assert!(meta.highlight_lines.contains_one_based(1));
    assert_eq!(meta.annotations.queries.len(), 1);
}

#[test]
fn javascript_twoslash_directives_completions_cuts_and_builtin_inference() {
    let md = "\
```ts twoslash title=\"src/index.ts\"
// @errors: 2322
// @filename: types.ts
interface User {
  id: number
  name: string
}
// ---cut---
// @filename: index.ts
// [!code word:total]
// [!code highlight:2]
/** Counts active names. */
function countNames(items: string[]) {
  return items.length
}
// ---cut-start---
const secret = 42
// ---cut-end---
const total = countNames([\"Ada\", \"Grace\"])
//    ^?
const bad: number = \"oops\"
total.toF
//       ^|
```
";
    let mut doc = Document::new(Schema::builtin());
    doc.insert_markdown(None, 0, md).unwrap();
    let meta = CodeBlockMeta::of(&doc, doc.blocks()[0]).expect("meta");
    let ann = &meta.annotations;

    assert_eq!(ann.expected_errors, vec!["2322".to_owned()]);
    assert_eq!(ann.files.len(), 2);
    assert_eq!(ann.files[0].name, "types.ts");
    assert_eq!(ann.files[1].name, "index.ts");

    // Bare `^?` inferred `const total: number` from `countNames`'s return expression.
    assert_eq!(ann.queries.len(), 1);
    assert_eq!(ann.queries[0].symbol, "total");
    assert_eq!(ann.queries[0].text.as_deref(), Some("const total: number"));

    // Bare `^|` resolved `number` methods matching prefix `toF` -> `toFixed`.
    assert_eq!(ann.completions.len(), 1);
    assert_eq!(ann.completions[0].receiver.as_deref(), Some("total"));
    assert_eq!(ann.completions[0].prefix, "toF");
    assert_eq!(ann.completions[0].items[0].label, "toFixed");

    // `@errors: 2322` retained the TS2322 diagnostic on `const bad: number = "oops"`.
    assert_eq!(ann.diagnostics.len(), 1);
    assert_eq!(ann.diagnostics[0].code.as_deref(), Some("2322"));

    // JSDoc hover was automatically attached to `countNames`.
    let fn_hover = ann
        .hovers
        .iter()
        .find(|h| h.symbol == "countNames")
        .expect("countNames hover");
    assert_eq!(
        fn_hover.signature,
        "function countNames(items: string[]): number"
    );
    assert_eq!(fn_hover.docs.as_deref(), Some("Counts active names."));

    // Word highlight and multi-line `[!code highlight:2]` were extracted.
    assert!(!ann.word_highlights.is_empty());
    assert!(
        ann.lines
            .iter()
            .filter(|m| m.tag == LineTag::Highlight)
            .count()
            >= 2
    );

    // Copy strips `@errors`, `@filename`, `---cut---`, `---cut-start/end---`, `[!code ...]`, `^?`, and `^|`.
    let raw = doc.plain_text(doc.blocks()[0]).unwrap();
    let copied = clean_copy_text(&raw, true);
    assert!(!copied.contains("interface User"));
    assert!(!copied.contains("secret"));
    assert!(!copied.contains("^?"));
    assert!(!copied.contains("^|"));
    assert!(!copied.contains("[!code"));
    assert!(copied.contains("function countNames(items: string[])"));
}

#[test]
fn inline_code_tags_require_comment_markers_and_handle_html_comments() {
    let strings = "const doc = \"[!code highlight]\";\nconst raw = \"[!code custom]\";";
    let parsed = parse_code_annotations(strings, None, true);
    assert!(parsed.lines.is_empty());
    assert!(parsed.trimmed_in_preview.is_empty());
    assert_eq!(clean_copy_text(strings, false), strings);
    assert_eq!(clean_copy_text(strings, true), strings);

    let unknown_comment = "// [!code custom]\nlet ok = 1;";
    assert_eq!(clean_copy_text(unknown_comment, true), unknown_comment);

    let html = "<!-- [!code highlight] -->\n<div>Hi</div> <!-- [!code ++] -->";
    let html_parsed = parse_code_annotations(html, None, false);
    assert_eq!(html_parsed.hidden_in_preview, vec![0]);
    assert!(
        html_parsed
            .lines
            .iter()
            .any(|m| m.line == 1 && m.tag == LineTag::Added)
    );
    assert_eq!(clean_copy_text(html, false), "<div>Hi</div>");

    let block_comment = "/* [!code highlight] */\nlet kept = 2; /* [!code --] */";
    let block_parsed = parse_code_annotations(block_comment, None, false);
    assert_eq!(block_parsed.hidden_in_preview, vec![0]);
    assert_eq!(clean_copy_text(block_comment, false), "let kept = 2;");
}

#[test]
fn inline_error_directive_deduplicates_overlapping_analyzer_diagnostic() {
    let code = "const bad: number = \"oops\";\n//                  ^^^^^^ @error(2322): Custom inline error";
    let mut parsed = parse_code_annotations(code, None, true);
    assert_eq!(parsed.diagnostics.len(), 1);

    let req = lotype_code::AnalysisRequest {
        language: Some("ts".into()),
        title: None,
        code: code.to_owned(),
        queries: parsed.query_sites(),
        completions: parsed.completion_sites(),
        files: parsed.files.clone(),
        no_errors: parsed.no_errors,
        expected_errors: parsed.expected_errors.clone(),
    };
    let analysis = lotype_code::BuiltInAnalyzer::analyze_sync(&req);
    assert!(
        !analysis.diagnostics.is_empty(),
        "BuiltInAnalyzer should produce a TS2322 diagnostic"
    );

    parsed.merge_analysis(&analysis);
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(parsed.diagnostics[0].message, "Custom inline error");
}

#[test]
fn crlf_input_preserves_queries_completions_virtual_files_and_clean_copy() {
    let code = concat!(
        "// @filename: a.ts\r\n",
        "const total = 42;\r\n",
        "//    ^?\r\n",
        "// @filename: b.ts\r\n",
        "total.toF\r\n",
        "//       ^|\r\n",
        "const kept = 1; // [!code ++]\r\n",
    );
    let mut parsed = parse_code_annotations(code, None, true);

    assert_eq!(parsed.queries.len(), 1);
    assert_eq!(parsed.queries[0].symbol, "total");
    assert_eq!(&code[parsed.queries[0].target_range.clone()], "total");

    assert_eq!(parsed.completions.len(), 1);
    assert_eq!(parsed.completions[0].receiver.as_deref(), Some("total"));
    assert_eq!(parsed.completions[0].prefix, "toF");
    let expected_end = code.find("total.toF").unwrap() + "total.toF".len();
    assert_eq!(parsed.completions[0].offset, expected_end);

    assert_eq!(parsed.files.len(), 2);
    assert_eq!(parsed.files[0].name, "a.ts");
    assert!(!code[parsed.files[0].range.clone()].starts_with('\n'));
    assert!(!code[parsed.files[0].range.clone()].ends_with('\r'));

    let req = lotype_code::AnalysisRequest {
        language: Some("ts".into()),
        title: None,
        code: code.to_owned(),
        queries: parsed.query_sites(),
        completions: parsed.completion_sites(),
        files: parsed.files.clone(),
        no_errors: parsed.no_errors,
        expected_errors: parsed.expected_errors.clone(),
    };
    parsed.merge_analysis(&lotype_code::BuiltInAnalyzer::analyze_sync(&req));
    assert_eq!(parsed.completions[0].items[0].label, "toFixed");

    let copied = clean_copy_text(code, true);
    assert!(!copied.contains('\r'), "CRLF residue in copied: {copied:?}");
    assert_eq!(copied, "const total = 42;\ntotal.toF\nconst kept = 1;\n");
}

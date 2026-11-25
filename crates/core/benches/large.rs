//! What editing costs in a large document: `cargo bench -p lotype-core`.
//! The limits these are held to are in ARCHITECTURE.md, with the milestones.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use lotype_core::{Document, Editor, Pos, Schema, Selection};

fn paragraphs(count: usize, word: &str) -> String {
    (0..count).map(|at| format!("{word} {at}\n\n")).collect()
}

/// A document of `count` paragraphs, with a removed block among its top
/// blocks as any document that was edited has.
fn document(count: usize) -> Document {
    let mut doc = Document::new(Schema::builtin());
    let gone = doc.insert_block(None, 0, "paragraph").unwrap();
    doc.remove_block(gone).unwrap();
    doc.insert_markdown(None, 0, &paragraphs(count, "paragraph"))
        .unwrap();
    doc
}

/// An editor of such a document, with the caret in the middle of it.
fn editor(count: usize) -> Editor {
    let doc = document(count);
    let middle = doc.blocks()[count / 2];
    let mut editor = Editor::new(doc);
    let caret = Selection::caret(Pos::new(middle, 3));
    editor.set_selection(Some(caret)).unwrap();
    editor
}

fn large(c: &mut Criterion) {
    let mut group = c.benchmark_group("large");
    group.sample_size(10);

    let text = paragraphs(4_000, "new");
    group.bench_function("markdown: 4,000 blocks into the middle of 16,000", |b| {
        // What it is given goes back, so that dropping it is not timed.
        let read = |mut doc: Document| {
            doc.insert_markdown(None, 8_000, black_box(&text)).unwrap();
            doc
        };
        b.iter_batched(|| document(16_000), read, BatchSize::PerIteration);
    });

    let mut typing = editor(5_000);
    group.bench_function("typing: a character at 5,000 paragraphs", |b| {
        b.iter(|| typing.insert_text(black_box("a")).unwrap());
    });

    // An editor that has run a command has its draft to run the next in.
    let warm = |count| {
        let mut editor = editor(count);
        editor.split_block().unwrap();
        editor
    };
    group.bench_function("enter: a new block at 5,000 paragraphs", |b| {
        let enter = |mut editor: Editor| {
            editor.split_block().unwrap();
            editor
        };
        b.iter_batched(|| warm(5_000), enter, BatchSize::PerIteration);
    });

    // Blocks made after other edits of the command: the draft is made anew.
    let pasted = paragraphs(20, "pasted");
    group.bench_function("paste: 20 blocks at 5,000 paragraphs", |b| {
        let paste = |mut editor: Editor| {
            editor.paste_markdown(black_box(&pasted)).unwrap();
            editor
        };
        b.iter_batched(|| warm(5_000), paste, BatchSize::PerIteration);
    });
    group.finish();
}

criterion_group!(benches, large);
criterion_main!(benches);

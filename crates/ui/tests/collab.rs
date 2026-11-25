use gpui::TestAppContext;
use lotype_core::test_support::editor;
use lotype_core::{Document, Pos, Presence, Schema, Selection, blocks, p};
use lotype_ui::EditorView;
use lotype_ui::test_support::EditorTestContext;

#[gpui::test]
fn drawing_peers_does_not_fail_whatever_they_call_themselves(cx: &mut TestAppContext) {
    let local = editor(blocks![p!["one<a>"], p!["two"]]);
    let mut peer = Document::new(Schema::builtin());
    peer.import(&local.document().export_snapshot().unwrap())
        .unwrap();
    let blocks = peer.blocks();
    let range = Selection::Text {
        anchor: Pos::new(blocks[0], 1),
        head: Pos::new(blocks[1], 2),
    };
    let presence = Presence::new();
    let shown = presence.clone();
    let mut ed =
        EditorTestContext::with_view(cx, |cx| EditorView::new(local, cx).with_presence(shown));

    for name in [
        "Ann",
        "",
        "two\nlines\tand a name far too long to show whole",
    ] {
        let sent = Presence::publish(&peer, name, 0xff_8800, Some(&range));
        presence.apply(&sent).unwrap();
        ed.view.update(ed.cx, |_, cx| cx.notify());
        ed.settle();
        let seen = ed.editor(|editor| presence.collaborators(editor.document()));
        assert_eq!(seen[0].selection.as_ref(), Some(&range));
    }
    // Typing before a peer's selection moves it along.
    ed.keys("cmd-up");
    ed.type_text("zero ");
    let seen = ed.editor(|editor| presence.collaborators(editor.document()));
    let Some(Selection::Text { anchor, .. }) = seen[0].selection else {
        panic!("the peer has text selected");
    };
    assert_eq!(anchor, Pos::new(blocks[0], 6));
}

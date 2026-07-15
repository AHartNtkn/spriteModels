use desktop_app::{
    ShellState,
    menu::{MenuAction, MenuGroup, PendingDestructiveAction, UnsavedChoice, menu_items},
};
use editor_core::EditorDocument;
use relief_core::{Bounds, CanonicalView};

fn document() -> EditorDocument {
    EditorDocument::new(Bounds::new(4, 3, 2).unwrap(), CanonicalView::Front)
}

fn dirty_document() -> EditorDocument {
    let mut document = document();
    document
        .set_source_opposite(CanonicalView::Front, false)
        .unwrap();
    document.add_source(CanonicalView::Back).unwrap();
    document
}

#[test]
fn top_menu_labels_map_to_the_complete_approved_action_set() {
    let labels_and_actions = |group| {
        menu_items(group)
            .iter()
            .map(|item| (item.label, item.action))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        labels_and_actions(MenuGroup::File),
        vec![
            ("New", MenuAction::New),
            ("Open", MenuAction::Open),
            ("Save", MenuAction::Save),
            ("Save As", MenuAction::SaveAs),
            ("Quit", MenuAction::Quit),
        ]
    );
    assert_eq!(
        labels_and_actions(MenuGroup::Edit),
        vec![("Undo", MenuAction::Undo), ("Redo", MenuAction::Redo)]
    );
    assert_eq!(
        labels_and_actions(MenuGroup::View),
        vec![("Reset Model View", MenuAction::ResetView)]
    );
}

#[test]
fn dirty_destructive_action_waits_for_an_unsaved_choice_and_cancel_clears_it() {
    let mut shell = ShellState::new(dirty_document());

    shell.request_destructive(PendingDestructiveAction::New);
    assert_eq!(
        shell.pending_destructive_action(),
        Some(&PendingDestructiveAction::New)
    );
    assert!(shell.document().is_dirty());

    shell.resolve_unsaved(UnsavedChoice::Cancel, None);
    assert_eq!(shell.pending_destructive_action(), None);
    assert!(shell.document().is_dirty());
    assert_eq!(shell.document().sources().len(), 2);
}

#[test]
fn discard_completes_a_pending_new_action() {
    let mut shell = ShellState::new(dirty_document());
    shell.request_destructive(PendingDestructiveAction::New);

    shell.resolve_unsaved(UnsavedChoice::Discard, None);

    assert_eq!(shell.pending_destructive_action(), None);
    assert!(!shell.document().is_dirty());
    assert_eq!(shell.document().sources().len(), 1);
    assert_eq!(shell.document().selected_view(), CanonicalView::Front);
    assert!(!shell.quit_requested());
}

#[test]
fn save_completes_the_pending_action_only_after_the_save_succeeds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saved.depthsprite");
    let mut shell = ShellState::new(dirty_document());
    shell.request_destructive(PendingDestructiveAction::Quit);

    shell.resolve_unsaved(UnsavedChoice::Save, Some(path.clone()));

    assert_eq!(shell.pending_destructive_action(), None);
    assert!(!shell.document().is_dirty());
    assert_eq!(shell.document().path(), Some(path.as_path()));
    assert!(shell.quit_requested());
    assert!(shell.file_error().is_none());
}

#[test]
fn failed_open_retains_the_current_document_and_reports_a_dismissible_error() {
    let directory = tempfile::tempdir().unwrap();
    let mut shell = ShellState::new(dirty_document());
    let before_bounds = shell.document().bounds();
    let before_sources = shell.document().sources().count();
    let missing = directory.path().join("missing.depthsprite");
    assert!(!missing.exists());

    shell.request_destructive(PendingDestructiveAction::Open(missing));
    shell.resolve_unsaved(UnsavedChoice::Discard, None);

    assert_eq!(shell.document().bounds(), before_bounds);
    assert_eq!(shell.document().sources().count(), before_sources);
    assert!(shell.document().is_dirty());
    assert_eq!(shell.pending_destructive_action(), None);
    assert!(shell.file_error().is_some());

    shell.dismiss_file_error();
    assert!(shell.file_error().is_none());
}

#[test]
fn a_failed_save_keeps_the_pending_action_and_current_document() {
    let directory = tempfile::tempdir().unwrap();
    let invalid_path = directory.path().join("missing-parent/model.depthsprite");
    let mut shell = ShellState::new(dirty_document());
    shell.request_destructive(PendingDestructiveAction::Quit);

    shell.resolve_unsaved(UnsavedChoice::Save, Some(invalid_path));

    assert_eq!(
        shell.pending_destructive_action(),
        Some(&PendingDestructiveAction::Quit)
    );
    assert!(shell.document().is_dirty());
    assert!(!shell.quit_requested());
    assert!(shell.file_error().is_some());
}

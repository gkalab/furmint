use super::*;

#[test]
fn test_show_hidden_defaults_to_true() {
    // The documented default, and what a first run or a legacy state file gets.
    let app = app_with(listing());
    assert!(app.show_hidden);
    assert_eq!(
        names(app.active_tab()),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );
}

#[test]
fn test_hidden_gate_excludes_dotfiles_and_dot_directories() {
    let mut tab = tab_with(listing());
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );

    tab.set_show_hidden(false);
    assert_eq!(names(&tab), vec!["..", "docs", "README.md"]);
}

#[test]
fn test_parent_entry_is_never_hidden() {
    let mut tab = tab_with(listing());
    tab.set_show_hidden(false);
    assert!(tab.is_visible(0));
    assert_eq!(tab.entries[0].name, "..");
}

#[test]
fn test_toggle_back_restores_hidden_entries() {
    let mut tab = tab_with(listing());
    tab.set_show_hidden(false);
    tab.set_show_hidden(true);
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );
}

#[test]
fn test_hidden_gate_delegates_to_the_shared_is_hidden_predicate() {
    // The filesystem-dependent difference (leading dot on Unix filesystems, DOS
    // attribute on a local Windows one) must stay in `fs::utils::is_hidden`
    // rather than being reimplemented here.
    let entries = vec![
        entry("..", true, ""),
        // Hidden through the leading dot, which only counts on Unix filesystems.
        entry(".dotfile", false, "-rw-r--r--"),
        // Hidden through the attribute string, which is what counts on Windows.
        entry("flagged", false, hidden_attrs(false)),
        entry("plain.txt", false, "-rw-r--r--"),
    ];
    let mut tab = tab_with(entries);
    let unix_attributes = crate::fs::utils::has_unix_attributes(tab.provider.is_local());
    tab.set_show_hidden(false);

    for i in 0..tab.entries.len() {
        let e = &tab.entries[i];
        assert_eq!(
            tab.is_visible(i),
            e.name == ".." || !crate::fs::utils::is_hidden(&e.name, &e.attributes, unix_attributes),
            "visibility of {:?} disagrees with is_hidden",
            e.name
        );
    }
}

#[test]
fn test_visibility_tracks_both_settings() {
    let mut tab = tab_with(listing());
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );

    tab.set_show_hidden(false);
    assert_eq!(names(&tab), vec!["..", "docs", "README.md"]);

    tab.set_show_hidden(true);
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );

    tab.set_file_filter(Some("README")).unwrap();
    assert_eq!(names(&tab), vec!["..", ".config", "docs", "README.md"]);
}

#[test]
fn test_toggle_hides_dotfiles_in_a_remote_tab_on_every_platform() {
    // Regression: hidden-ness was decided by `cfg!(windows)` on the client, so
    // from Windows a remote tab withheld nothing at all — the toggle reported
    // "Hidden files hidden" while every remote dotfile stayed listed. A remote
    // listing reports Unix permissions and carries no DOS flags, so the dot
    // convention must apply there regardless of where the app runs.
    let mut tab = remote_tab_with(remote_listing());
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );

    tab.set_show_hidden(false);
    assert_eq!(names(&tab), vec!["..", "docs", "README.md"]);

    tab.set_show_hidden(true);
    assert_eq!(
        names(&tab),
        vec!["..", ".config", "docs", ".env", "README.md"]
    );
}

#[test]
fn test_local_windows_tab_still_ignores_dot_prefixed_names() {
    // The mirror image of the remote case: a local Windows filesystem has no
    // dotfile convention, so an unflagged `.env` stays listed there.
    if !cfg!(windows) {
        return;
    }
    let mut tab = tab_with(vec![
        entry("..", true, ""),
        entry(".env", false, "------"),
        entry("README.md", false, "------"),
    ]);
    tab.set_show_hidden(false);
    assert_eq!(names(&tab), vec!["..", ".env", "README.md"]);
}

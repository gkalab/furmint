use super::*;

fn size(w: u16, h: u16) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: w,
        height: h,
    }
}

#[test]
fn panels_split_50_50_with_status_row() {
    let tab = crate::test_utils::create_test_tab();
    let layout = compute_layout(
        size(100, 30),
        std::slice::from_ref(&tab),
        std::slice::from_ref(&tab),
        None,
        false,
    );

    assert_eq!(
        layout.left_panel_area,
        Rect {
            x: 0,
            y: 0,
            width: 50,
            height: 29
        }
    );
    assert_eq!(
        layout.right_panel_area,
        Rect {
            x: 50,
            y: 0,
            width: 50,
            height: 29
        }
    );
    assert_eq!(layout.left_tab_bar_area, Rect::default());
    assert_eq!(layout.right_tab_bar_area, Rect::default());
    assert_eq!(layout.left_tab_areas, Vec::new());
    assert_eq!(layout.right_tab_areas, Vec::new());
}

#[test]
fn tab_bar_shown_only_with_multiple_tabs() {
    let tab = crate::test_utils::create_test_tab();
    let two = vec![tab.clone(), tab.clone()];

    // show_tabs is global: a side with a single tab still gets a bar
    let with_one = compute_layout(size(100, 30), &[tab], &two, None, false);
    assert_eq!(
        with_one.left_tab_bar_area,
        Rect {
            x: 0,
            y: 0,
            width: 50,
            height: 1
        }
    );
    assert_eq!(with_one.left_tab_areas.len(), 1);

    let with_two = compute_layout(size(100, 30), &two, &two, None, false);
    assert_eq!(
        with_two.left_tab_bar_area,
        Rect {
            x: 0,
            y: 0,
            width: 50,
            height: 1
        }
    );
    assert_eq!(
        with_two.left_panel_area,
        Rect {
            x: 0,
            y: 1,
            width: 50,
            height: 28
        }
    );
    assert_eq!(with_two.left_tab_areas.len(), 2);
}

#[test]
fn viewer_side_loses_its_tab_bar_and_panels_cover_the_side() {
    let tab = crate::test_utils::create_test_tab();
    let two = vec![tab.clone(), tab.clone()];

    let layout = compute_layout(
        size(100, 30),
        &two,
        std::slice::from_ref(&tab),
        Some(PanelSide::Right),
        false,
    );

    assert_eq!(
        layout.right_panel_area,
        Rect {
            x: 50,
            y: 0,
            width: 50,
            height: 29
        }
    );
    assert_eq!(layout.right_tab_bar_area, Rect::default());
    assert_eq!(layout.right_tab_areas, Vec::new());
    // Left side still has its tab bar
    assert_eq!(
        layout.left_tab_bar_area,
        Rect {
            x: 0,
            y: 0,
            width: 50,
            height: 1
        }
    );
}

#[test]
fn tab_areas_follow_titles_and_separators() {
    let mut a = crate::test_utils::create_test_tab();
    a.custom_title = Some("ab".to_string());
    let mut b = crate::test_utils::create_test_tab();
    b.custom_title = Some("cdef".to_string());

    let bar = Rect {
        x: 10,
        y: 0,
        width: 50,
        height: 1,
    };
    let areas = compute_tab_areas(bar, &[a.clone(), b], false);

    // "ab" -> width 4 (" ab "), separator, then "cdef" -> width 6
    assert_eq!(
        areas,
        vec![
            Rect {
                x: 10,
                y: 0,
                width: 4,
                height: 1
            },
            Rect {
                x: 15,
                y: 0,
                width: 6,
                height: 1
            },
        ]
    );
    assert_eq!(
        compute_tab_areas(
            Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 0
            },
            &[a],
            false
        ),
        Vec::<Rect>::new()
    );
}

#[test]
fn tab_width_includes_icons() {
    let tab = crate::test_utils::create_test_tab();
    // " test " = 6 columns; icons add left/right edges
    assert_eq!(tab_area_width(&tab, false), 6);
    assert_eq!(tab_area_width(&tab, true), 8);
}

#[test]
fn ascii_title_truncation_unchanged() {
    let mut tab = crate::test_utils::create_test_tab();
    tab.current_dir = std::path::PathBuf::from("abcdefghijklmnop"); // 16 > 15
    assert_eq!(tab_display_title(&tab), "abcdefghijkl…");
}

#[test]
fn multibyte_title_truncation_does_not_panic() {
    // 18 display columns: 3 CJK (3 each... 2 cols) + 8 é + "test".
    // Old code sliced bytes at `&title[..12]`, which lands inside the
    // second é (byte 12 is not a char boundary) and panicked on draw.
    let mut tab = crate::test_utils::create_test_tab();
    tab.current_dir = std::path::PathBuf::from("日本語éééééééétest");

    let title = tab_display_title(&tab);
    assert_eq!(title, "日本語éééééé…");
    assert!(display_width(&title) <= 15);
}

#[test]
fn multibyte_title_width_is_display_width_aware() {
    let mut tab = crate::test_utils::create_test_tab();
    tab.current_dir = std::path::PathBuf::from("日本語");
    // Width 6, not the 3 chars the old code counted
    assert_eq!(display_width("日本語"), 6);
    assert_eq!(tab_display_title(&tab), "日本語");
    // 6 display columns + 2 padding
    assert_eq!(tab_area_width(&tab, false), 8);

    // Wide title exceeding the 15-column local budget truncates by width
    tab.current_dir = std::path::PathBuf::from("日本語日本語日本語xx");
    let title = tab_display_title(&tab);
    assert!(title.ends_with('…'));
    assert_eq!(display_width(&title), 13); // 12 + ellipsis
    assert_eq!(tab_area_width(&tab, false), 15);
}

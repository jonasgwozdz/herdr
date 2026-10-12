use super::*;
use crate::protocol::{color_to_u32, PaneSurfacePatch, PaneSurfacePatchRow};
use ratatui::style::Color;

fn colored_config() -> Config {
    toml::from_str("[ui.pane_colors]\nautomatic = true").unwrap()
}

#[test]
fn pane_colors_preserve_child_cells_and_raw_surface() {
    let mut raw = surface();
    raw.frame.cells[1].fg = color_to_u32(Color::Rgb(255, 85, 85));
    raw.frame.cells[1].bg = color_to_u32(Color::Indexed(25));
    raw.frame.cells[1].hyperlink = Some(0);
    raw.frame.hyperlinks.push("https://example.com".into());
    raw.frame.cells[1].modifier = Modifier::ITALIC.bits();
    let expected = raw.clone();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(raw);
    let composed = state.compose(106, 20).unwrap();
    let hit = &state.hits.panes[0];
    let index = usize::from(hit.inner_rect.y) * 106 + usize::from(hit.inner_rect.x);
    assert_ne!(composed.cells[index].bg, 0);
    assert_eq!(composed.cells[index + 1].fg, expected.frame.cells[1].fg);
    assert_eq!(composed.cells[index + 1].bg, expected.frame.cells[1].bg);
    assert_eq!(composed.cells[index + 1].modifier, Modifier::ITALIC.bits());
    assert_eq!(composed.cells[index + 1].hyperlink, Some(0));
    assert_eq!(state.pane_surface.as_ref().unwrap(), &expected);
}

#[test]
fn pane_colors_incremental_patch_matches_full_compose() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let initial = state.compose(106, 20).unwrap();
    let current = state.pane_surface.as_ref().unwrap();
    let mut pane = current.panes[0].clone();
    pane.content_revision += 1;
    let mut cells = current.frame.cells[..4].to_vec();
    cells[0].symbol = "X".into();
    let patch = PaneSurfacePatch {
        boot_id: current.boot_id.clone(),
        projection_revision: current.projection_revision,
        base_surface_revision: current.surface_revision,
        surface_revision: current.surface_revision + 1,
        rows: vec![PaneSurfacePatchRow { x: 0, y: 0, cells }],
        panes: vec![pane],
        cursor: current.frame.cursor.clone(),
    };
    let ClientPaneSurfacePatchOutcome::Applied(Some(patch)) = state.apply_pane_surface_patch(patch)
    else {
        panic!("pane colors must preserve the incremental fast path");
    };
    let patched = apply_composed_surface_patch(&initial, patch).unwrap();
    let full = state.compose(106, 20).unwrap();
    assert_eq!(patched.cells, full.cells);
    assert_eq!(state.pane_surface.as_ref().unwrap().frame.cells[0].bg, 0);
}

#[test]
fn pane_colors_reload_and_disable_restore_server_colors() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let tinted = state.compose(106, 20).unwrap();
    let hit = state.hits.panes[0].clone();
    let index = usize::from(hit.inner_rect.y) * 106 + usize::from(hit.inner_rect.x);
    let config: Config = toml::from_str(
        r##"
[[ui.pane_colors.rules]]
pane = "pane_1"
background = "#123456"
"##,
    )
    .unwrap();
    state.config.apply_live_config(&config, &[], &[]);
    let replaced = state.compose(106, 20).unwrap();
    assert_eq!(
        replaced.cells[index].bg,
        color_to_u32(Color::Rgb(0x12, 0x34, 0x56))
    );
    assert_ne!(tinted.cells[index].bg, replaced.cells[index].bg);
    state.config.apply_live_config(&Config::default(), &[], &[]);
    let restored = state.compose(106, 20).unwrap();
    assert_eq!(restored.cells[index].bg, 0);
}

#[test]
fn pane_colors_rules_prefer_id_over_manual_label() {
    let config: Config = toml::from_str(
        r##"
[[ui.pane_colors.rules]]
pane = "reviewer"
background = "#112233"
[[ui.pane_colors.rules]]
pane = "pane_1"
background = "#445566"
"##,
    )
    .unwrap();
    let mut snap = snapshot();
    snap.panes[0].label = Some("reviewer".into());
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snap.clone()));
    state.set_pane_surface(surface());
    let full = state.compose(106, 20).unwrap();
    let hit = state.hits.panes[0].clone();
    let index = usize::from(hit.inner_rect.y) * 106 + usize::from(hit.inner_rect.x);
    assert_eq!(
        full.cells[index].bg,
        color_to_u32(Color::Rgb(0x44, 0x55, 0x66))
    );
    snap.panes[0].pane_id = "moved".into();
    snap.focused_pane_id = Some("moved".into());
    let mut moved = surface();
    moved.panes[0].pane_id = "moved".into();
    state.set_snapshot(Box::new(snap));
    state.set_pane_surface(moved);
    let full = state.compose(106, 20).unwrap();
    assert_eq!(
        full.cells[index].bg,
        color_to_u32(Color::Rgb(0x11, 0x22, 0x33))
    );
}

#[test]
fn pane_colors_automatic_panes_and_focused_border_are_distinct() {
    let mut colors = surface();
    colors.frame = FrameData::from_ratatui_buffer_with_hyperlinks(
        &Buffer::empty(Rect::new(0, 0, 8, 3)),
        None,
        &[],
    );
    let mut first = colors.panes[0].clone();
    first.pane_id = "w1:p1".into();
    first.rect = SurfaceRect {
        x: 0,
        y: 0,
        width: 4,
        height: 3,
    };
    first.inner_rect = SurfaceRect {
        x: 1,
        y: 1,
        width: 2,
        height: 1,
    };
    let mut second = first.clone();
    second.pane_id = "w1:p2".into();
    // Independent bordered panes have adjacent rectangles in the server layout.
    second.rect.x = 4;
    second.inner_rect.x = 5;
    second.focused = false;
    colors.frame.cells[0].symbol = "┌".into();
    colors.frame.cells[4].symbol = "┌".into();
    colors.panes = vec![first, second];
    let original = colors.clone();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(colors);
    let full = state.compose(106, 20).unwrap();
    let a = &state.hits.panes[0];
    let b = &state.hits.panes[1];
    let index = |x, y| usize::from(y) * 106 + usize::from(x);
    assert_ne!(
        full.cells[index(a.inner_rect.x, a.inner_rect.y)].bg,
        full.cells[index(b.inner_rect.x, b.inner_rect.y)].bg
    );
    let left = &full.cells[index(a.rect.x, a.rect.y)];
    let right = &full.cells[index(b.rect.x, b.rect.y)];
    assert_ne!(left.fg, right.fg);
    assert_ne!(left.modifier & Modifier::BOLD.bits(), 0);
    assert_eq!(right.modifier & Modifier::BOLD.bits(), 0);
    assert_eq!(state.pane_surface.as_ref().unwrap(), &original);
}

#[test]
#[ignore = "manual client pane-color scaling profile"]
fn pane_color_scaling_profile() {
    use std::{hint::black_box, time::Instant};
    for count in [1, 15] {
        let mut raw = surface();
        raw.frame = FrameData::from_ratatui_buffer_with_hyperlinks(
            &Buffer::empty(Rect::new(0, 0, 180, 60)),
            None,
            &[],
        );
        let template = raw.panes[0].clone();
        raw.panes = (0..count)
            .map(|i| {
                let mut pane = template.clone();
                pane.pane_id = format!("w1:p{}", i + 1);
                pane.rect = SurfaceRect {
                    x: i * (180 / count),
                    y: 0,
                    width: 180 / count,
                    height: 60,
                };
                pane.inner_rect = pane.rect;
                pane
            })
            .collect();
        let snap = snapshot();
        let area = Rect::new(0, 0, 180, 60);
        let mut frame = raw.frame.clone();
        for automatic in [false, true] {
            let config: Config =
                toml::from_str(&format!("[ui.pane_colors]\nautomatic = {automatic}")).unwrap();
            let colors = super::super::pane_colors::PaneColors::from_config(&config.ui.pane_colors);
            let mut elapsed = std::time::Duration::ZERO;
            for _ in 0..1000 {
                frame.clone_from(&raw.frame);
                let start = Instant::now();
                colors.paint_frame(
                    black_box(&mut frame),
                    black_box(&raw),
                    &snap,
                    area,
                    color_to_u32(Palette::catppuccin().accent),
                );
                elapsed += start.elapsed();
            }
            println!(
                "pane_colors panes={count} enabled={automatic} ns/frame={}",
                elapsed.as_nanos() / 1000
            );
        }
    }
}

#[test]
fn pane_colors_scrollbar_visibility_patch_uses_updated_metadata() {
    for visible in [true, false] {
        let mut raw = surface();
        raw.panes[0].inner_rect.width = 3;
        let scrollbar = SurfaceRect {
            x: 3,
            y: 0,
            width: 1,
            height: 2,
        };
        raw.panes[0].scrollbar_rect = (!visible).then_some(scrollbar);
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
        state.set_snapshot(Box::new(snapshot()));
        state.set_pane_surface(raw);
        let initial = state.compose(106, 20).unwrap();
        let current = state.pane_surface.as_ref().unwrap();
        let mut pane = current.panes[0].clone();
        pane.scrollbar_rect = visible.then_some(scrollbar);
        pane.content_revision += 1;
        let mut cell = current.frame.cells[3].clone();
        cell.symbol = if visible { "█" } else { " " }.into();
        cell.fg = color_to_u32(Color::Rgb(20, 30, 40));
        let patch = PaneSurfacePatch {
            boot_id: current.boot_id.clone(),
            projection_revision: current.projection_revision,
            base_surface_revision: current.surface_revision,
            surface_revision: current.surface_revision + 1,
            rows: vec![PaneSurfacePatchRow {
                x: 3,
                y: 0,
                cells: vec![cell],
            }],
            panes: vec![pane],
            cursor: current.frame.cursor.clone(),
        };
        let ClientPaneSurfacePatchOutcome::Applied(Some(patch)) =
            state.apply_pane_surface_patch(patch)
        else {
            panic!("scrollbar transition must keep fast path");
        };
        let patched = apply_composed_surface_patch(&initial, patch).unwrap();
        let full = state.compose(106, 20).unwrap();
        assert_eq!(patched.cells, full.cells, "visible={visible}");
    }
}

#[test]
fn pane_colors_shared_divider_keeps_neighbor_focus_visible() {
    let mut raw = surface();
    raw.frame = FrameData::from_ratatui_buffer_with_hyperlinks(
        &Buffer::empty(Rect::new(0, 0, 8, 3)),
        None,
        &[],
    );
    let mut left = raw.panes[0].clone();
    left.pane_id = "w1:p1".into();
    left.rect = SurfaceRect {
        x: 0,
        y: 0,
        width: 4,
        height: 3,
    };
    left.inner_rect = SurfaceRect {
        x: 0,
        y: 0,
        width: 4,
        height: 3,
    };
    let mut right = left.clone();
    right.pane_id = "w1:p2".into();
    right.focused = false;
    right.rect.x = 4;
    right.inner_rect.x = 5;
    right.inner_rect.width = 3;
    raw.frame.cells[12].symbol = "│".into();
    raw.frame.cells[12].fg = color_to_u32(Palette::catppuccin().accent);
    raw.panes = vec![left, right];
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(raw);
    let full = state.compose(106, 20).unwrap();
    let hit = &state.hits.panes[1];
    let index = usize::from(hit.rect.y + 1) * 106 + usize::from(hit.rect.x);
    assert_ne!(full.cells[index].modifier & Modifier::BOLD.bits(), 0);
}

#[test]
fn pane_colors_preserve_rgb_backgrounds_cursor_and_popups() {
    let mut raw = surface_with_popup();
    // OSC-modified defaults may arrive as explicit RGB, just like SGR backgrounds.
    raw.frame.cells[0].bg = color_to_u32(Color::Rgb(10, 20, 30));
    raw.frame.cursor = Some(crate::protocol::CursorState {
        x: 1,
        y: 1,
        visible: true,
        shape: 6,
    });
    let mut baseline = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    baseline.set_snapshot(Box::new(snapshot()));
    baseline.set_pane_surface(raw.clone());
    let plain = baseline.compose(106, 20).unwrap();
    let mut tinted = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
    tinted.set_snapshot(Box::new(snapshot()));
    tinted.set_pane_surface(raw);
    let colored = tinted.compose(106, 20).unwrap();
    assert_eq!(colored.cursor, plain.cursor);
    let popup = tinted.hits.popup.as_ref().unwrap().inner_rect;
    for y in popup.y..popup.bottom() {
        for x in popup.x..popup.right() {
            let i = usize::from(y) * 106 + usize::from(x);
            assert_eq!(colored.cells[i], plain.cells[i]);
        }
    }
    // Without the popup, preserve an application-owned RGB default background.
    let mut raw = surface();
    raw.frame.cells[0].bg = color_to_u32(Color::Rgb(10, 20, 30));
    raw.frame.cursor = Some(crate::protocol::CursorState {
        x: 1,
        y: 1,
        visible: true,
        shape: 6,
    });
    tinted.set_pane_surface(raw.clone());
    baseline.set_pane_surface(raw);
    let colored = tinted.compose(106, 20).unwrap();
    let plain = baseline.compose(106, 20).unwrap();
    let hit = &tinted.hits.panes[0];
    let i = usize::from(hit.inner_rect.y) * 106 + usize::from(hit.inner_rect.x);
    assert_eq!(colored.cells[i].bg, color_to_u32(Color::Rgb(10, 20, 30)));
    assert_eq!(colored.cursor, plain.cursor);
}

#[test]
fn pane_colors_agent_name_priority_and_duplicate_rules() {
    let config: Config = toml::from_str(
        r##"
[[ui.pane_colors.rules]]
pane = "reviewer"
background = "#112233"
[[ui.pane_colors.rules]]
pane = "reviewer"
background = "#223344"
[[ui.pane_colors.rules]]
pane = "manual"
background = "#334455"
"##,
    )
    .unwrap();
    let mut snap = snapshot();
    snap.agents.push(ClientShellAgent {
        pane_id: "pane_1".into(),
        workspace_id: "ws_1".into(),
        tab_id: "tab_1".into(),
        name: Some("reviewer".into()),
        display_agent: None,
        agent: Some("codex".into()),
        title: None,
        terminal_title: None,
        terminal_title_stripped: None,
        agent_status: AgentStatus::Idle,
        state_change_seq: 0,
        state_labels: Vec::new(),
        tokens: Vec::new(),
        focused: true,
    });
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snap.clone()));
    state.set_pane_surface(surface());
    let full = state.compose(106, 20).unwrap();
    let hit = state.hits.panes[0].clone();
    let index = usize::from(hit.inner_rect.y) * 106 + usize::from(hit.inner_rect.x);
    assert_eq!(
        full.cells[index].bg,
        color_to_u32(Color::Rgb(0x22, 0x33, 0x44))
    );
    snap.panes[0].label = Some("manual".into());
    state.set_snapshot(Box::new(snap));
    let full = state.compose(106, 20).unwrap();
    assert_eq!(
        full.cells[index].bg,
        color_to_u32(Color::Rgb(0x33, 0x44, 0x55))
    );
}

#[test]
fn automatic_nerv_palette_is_dark_and_cycles_through_public_pane_ids() {
    let colors =
        super::super::pane_colors::PaneColors::from_config(&colored_config().ui.pane_colors);
    let backgrounds = [0x080604, 0x04070b, 0x08050b, 0x040907, 0x0b0505, 0x040909];
    for number in 1..=70 {
        let pane_id = crate::workspace::public_pane_id_for_number("w1", number);
        let mut raw = surface();
        raw.panes[0].pane_id = pane_id;
        let mut frame = raw.frame.clone();
        colors.paint_frame(&mut frame, &raw, &snapshot(), Rect::new(0, 0, 4, 3), 0);
        assert_eq!(
            frame.cells[0].bg,
            0x02000000 | backgrounds[(number - 1) % 6]
        );
        assert_eq!(frame.cells[0].fg, 0x02ffc98a);
        assert_eq!(raw.frame.cells[0].bg, 0);
    }
}

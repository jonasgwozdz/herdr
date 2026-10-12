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
    let ClientPaneSurfacePatchOutcome::Applied(Some(patch)) = state.apply_pane_surface_patch(patch) else {
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
    let config: Config = toml::from_str(r##"
[[ui.pane_colors.rules]]
pane = "pane_1"
background = "#123456"
"##).unwrap();
    state.config.apply_live_config(&config, &[], &[]);
    let replaced = state.compose(106, 20).unwrap();
    assert_eq!(replaced.cells[index].bg, color_to_u32(Color::Rgb(0x12, 0x34, 0x56)));
    assert_ne!(tinted.cells[index].bg, replaced.cells[index].bg);
    state.config.apply_live_config(&Config::default(), &[], &[]);
    let restored = state.compose(106, 20).unwrap();
    assert_eq!(restored.cells[index].bg, 0);
}

#[test]
fn pane_colors_rules_prefer_id_over_manual_label() {
    let config: Config = toml::from_str(r##"
[[ui.pane_colors.rules]]
pane = "reviewer"
background = "#112233"
[[ui.pane_colors.rules]]
pane = "pane_1"
background = "#445566"
"##).unwrap();
    let mut snap = snapshot();
    snap.panes[0].label = Some("reviewer".into());
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snap.clone()));
    state.set_pane_surface(surface());
    let full = state.compose(106, 20).unwrap();
    let hit = state.hits.panes[0].clone();
    let index = usize::from(hit.inner_rect.y) * 106 + usize::from(hit.inner_rect.x);
    assert_eq!(full.cells[index].bg, color_to_u32(Color::Rgb(0x44, 0x55, 0x66)));
    snap.panes[0].pane_id = "moved".into();
    snap.focused_pane_id = Some("moved".into());
    let mut moved = surface();
    moved.panes[0].pane_id = "moved".into();
    state.set_snapshot(Box::new(snap));
    state.set_pane_surface(moved);
    let full = state.compose(106, 20).unwrap();
    assert_eq!(full.cells[index].bg, color_to_u32(Color::Rgb(0x11, 0x22, 0x33)));
}

#[test]
fn pane_colors_automatic_panes_and_focused_border_are_distinct() {
    let mut colors = surface();
    colors.frame = FrameData::from_ratatui_buffer_with_hyperlinks(
        &Buffer::empty(Rect::new(0, 0, 8, 3)), None, &[],
    );
    let mut first = colors.panes[0].clone();
    first.pane_id = "w1:p1".into();
    first.rect = SurfaceRect { x: 0, y: 0, width: 4, height: 3 };
    first.inner_rect = SurfaceRect { x: 1, y: 1, width: 2, height: 1 };
    let mut second = first.clone();
    second.pane_id = "w1:p2".into();
    second.rect.x = 4;
    second.inner_rect.x = 5;
    second.focused = false;
    colors.panes = vec![first, second];
    let original = colors.clone();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&colored_config()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(colors);
    let full = state.compose(106, 20).unwrap();
    let a = &state.hits.panes[0];
    let b = &state.hits.panes[1];
    let index = |x, y| usize::from(y) * 106 + usize::from(x);
    assert_ne!(full.cells[index(a.inner_rect.x, a.inner_rect.y)].bg,
        full.cells[index(b.inner_rect.x, b.inner_rect.y)].bg);
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
            &Buffer::empty(Rect::new(0, 0, 180, 60)), None, &[],
        );
        let template = raw.panes[0].clone();
        raw.panes = (0..count).map(|i| {
            let mut pane = template.clone();
            pane.pane_id = format!("w1:p{}", i + 1);
            pane.rect = SurfaceRect { x: i * (180 / count), y: 0, width: 180 / count, height: 60 };
            pane.inner_rect = pane.rect;
            pane
        }).collect();
        let snap = snapshot();
        let area = Rect::new(0, 0, 180, 60);
        let mut frame = raw.frame.clone();
        for automatic in [false, true] {
            let config: Config = toml::from_str(&format!("[ui.pane_colors]\nautomatic = {automatic}")).unwrap();
            let colors = super::super::pane_colors::PaneColors::from_config(&config.ui.pane_colors);
            let start = Instant::now();
            for _ in 0..1000 { colors.paint_frame(black_box(&mut frame), black_box(&raw), &snap, area); }
            println!("pane_colors panes={count} enabled={automatic} ns/frame={}", start.elapsed().as_nanos() / 1000);
        }
    }
}

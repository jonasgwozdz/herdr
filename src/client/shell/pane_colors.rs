//! Pane colors belong to the client. Apply them to outgoing cells without changing
//! the retained server surface, child terminal state, or endpoint codecs.
use std::collections::HashMap;

use crate::app::state::Palette;
use crate::config::PaneColorsConfig;
use crate::protocol::{
    color_to_u32, CellData, ClientShellSnapshot, FrameData, PaneSurfaceFrame, PaneSurfacePane,
    PaneSurfacePatchRow, SurfaceRect,
};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier},
};

#[derive(Clone, Copy, Default)]
struct PaneStyle {
    foreground: Option<u32>,
    background: Option<u32>,
    border: Option<u32>,
}

#[derive(Default)]
pub(super) struct PaneColors {
    automatic: bool,
    rules: HashMap<String, PaneStyle>,
}

// NERV variants keep the amber terminal text and nearly black backgrounds.
const AUTOMATIC: [(u32, u32); 6] = [
    (0x02080604, 0x02ff7a00),
    (0x0204070b, 0x022fa8ff),
    (0x0208050b, 0x029d5cff),
    (0x02040907, 0x0239ff6a),
    (0x020b0505, 0x02ff1e1e),
    (0x02040909, 0x0200e0b0),
];

fn automatic_style(pane_id: &str) -> PaneStyle {
    let number = pane_id
        .rsplit(":p")
        .next()
        .and_then(crate::workspace::decode_public_number);
    let index = number
        .map(|n| n.saturating_sub(1) % AUTOMATIC.len())
        .unwrap_or_else(|| {
            pane_id.bytes().fold(0usize, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(byte as usize)
            }) % AUTOMATIC.len()
        });
    let (background, border) = AUTOMATIC[index];
    PaneStyle {
        foreground: Some(0x02ffc98a),
        background: Some(background),
        border: Some(border),
    }
}

fn contains(rect: SurfaceRect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

impl PaneColors {
    pub(super) fn from_config(config: &PaneColorsConfig) -> Self {
        let rules = config
            .rules
            .iter()
            .map(|rule| {
                let mut style = rule
                    .theme
                    .as_deref()
                    .and_then(Palette::from_name)
                    .map(|p| PaneStyle {
                        foreground: Some(color_to_u32(p.text)),
                        background: Some(color_to_u32(p.panel_bg)),
                        border: Some(color_to_u32(p.accent)),
                    })
                    .unwrap_or_default();
                let color = |value: &str| color_to_u32(crate::config::parse_color(value));
                if let Some(value) = rule.foreground.as_deref() {
                    style.foreground = Some(color(value));
                }
                if let Some(value) = rule.background.as_deref() {
                    style.background = Some(color(value));
                }
                if let Some(value) = rule.border.as_deref() {
                    style.border = Some(color(value));
                }
                (rule.pane.clone(), style)
            })
            .collect();
        Self {
            automatic: config.automatic,
            rules,
        }
    }

    fn enabled(&self) -> bool {
        self.automatic || !self.rules.is_empty()
    }

    fn style(&self, snapshot: &ClientShellSnapshot, pane_id: &str) -> Option<PaneStyle> {
        if self.rules.is_empty() {
            return self.automatic.then(|| automatic_style(pane_id));
        }
        self.rules
            .get(pane_id)
            .copied()
            .or_else(|| {
                let label = snapshot
                    .panes
                    .iter()
                    .find(|p| p.pane_id == pane_id)?
                    .label
                    .as_deref()?;
                self.rules.get(label).copied()
            })
            .or_else(|| {
                let name = snapshot
                    .agents
                    .iter()
                    .find(|a| a.pane_id == pane_id)?
                    .name
                    .as_deref()?;
                self.rules.get(name).copied()
            })
            .or_else(|| self.automatic.then(|| automatic_style(pane_id)))
    }

    fn paint_cell(
        cell: &mut CellData,
        pane: &PaneSurfacePane,
        style: PaneStyle,
        x: u16,
        y: u16,
        focus_border_color: u32,
    ) {
        let source_fg = cell.fg;
        if cell.fg == color_to_u32(Color::Reset) {
            if let Some(fg) = style.foreground {
                cell.fg = fg;
            }
        }
        if cell.bg == color_to_u32(Color::Reset) {
            if let Some(bg) = style.background {
                cell.bg = bg;
            }
        }
        let border = !contains(pane.inner_rect, x, y)
            && !pane.scrollbar_rect.is_some_and(|rect| contains(rect, x, y))
            && (x == pane.rect.x
                || x == pane
                    .rect
                    .x
                    .saturating_add(pane.rect.width)
                    .saturating_sub(1)
                || y == pane.rect.y
                || y == pane
                    .rect
                    .y
                    .saturating_add(pane.rect.height)
                    .saturating_sub(1));
        if !border {
            return;
        }
        let frame_glyph = cell
            .symbol
            .chars()
            .any(|c| ('\u{2500}'..='\u{257f}').contains(&c));
        let top_title = y == pane.rect.y && pane.inner_rect.y > pane.rect.y;
        if frame_glyph || top_title {
            if let Some(color) = style.border {
                cell.fg = color;
                // The raw server surface already marks focused shared dividers
                // with the accent; preserve that signal without guessing ownership.
                if pane.focused || source_fg == focus_border_color {
                    cell.modifier |= Modifier::BOLD.bits();
                }
            }
        }
    }

    pub(super) fn paint_frame(
        &self,
        frame: &mut FrameData,
        surface: &PaneSurfaceFrame,
        snapshot: &ClientShellSnapshot,
        area: Rect,
        focus_border_color: u32,
    ) {
        if !self.enabled() {
            return;
        }
        for pane in &surface.panes {
            let Some(style) = self.style(snapshot, &pane.pane_id) else {
                continue;
            };
            for y in pane.rect.y
                ..pane
                    .rect
                    .y
                    .saturating_add(pane.rect.height)
                    .min(area.height)
            {
                for x in pane.rect.x..pane.rect.x.saturating_add(pane.rect.width).min(area.width) {
                    let tx = area.x.saturating_add(x);
                    let ty = area.y.saturating_add(y);
                    if tx >= frame.width || ty >= frame.height {
                        continue;
                    }
                    let index = usize::from(ty) * usize::from(frame.width) + usize::from(tx);
                    if let Some(cell) = frame.cells.get_mut(index) {
                        Self::paint_cell(cell, pane, style, x, y, focus_border_color);
                    }
                }
            }
        }
    }

    pub(super) fn paint_rows(
        &self,
        rows: &mut [PaneSurfacePatchRow],
        snapshot: &ClientShellSnapshot,
        area: Rect,
        updated_panes: &[PaneSurfacePane],
        focus_border_color: u32,
    ) {
        if !self.enabled() {
            return;
        }
        for row in rows {
            let Some(x) = row.x.checked_sub(area.x) else {
                continue;
            };
            let Some(y) = row.y.checked_sub(area.y) else {
                continue;
            };
            let Some(pane) = updated_panes.iter().find(|p| contains(p.rect, x, y)) else {
                continue;
            };
            let Some(style) = self.style(snapshot, &pane.pane_id) else {
                continue;
            };
            for (offset, cell) in row.cells.iter_mut().enumerate() {
                let Some(x) = u16::try_from(offset)
                    .ok()
                    .and_then(|offset| x.checked_add(offset))
                else {
                    break;
                };
                if contains(pane.rect, x, y) {
                    Self::paint_cell(cell, pane, style, x, y, focus_border_color);
                }
            }
        }
    }
}

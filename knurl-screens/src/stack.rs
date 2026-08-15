//! The scaffolding behind the two screens that draw their own scrolling stack.
//!
//! A hand-drawn page has no widget to scroll it: the screen keeps an offset (a
//! [`ScrollZone`](knurl::ScrollZone) on the chain moves it) and paints a window
//! of rows itself. This is that window - the clear, the row loop and the
//! overflow indicator - so the two screens are left with nothing but the rows
//! they actually draw.

use knurl::{Area, Component, RenderTarget, Scrollbar};

/// Pixels the scroll indicator wants on the right.
const BAR_W: u16 = 4;

/// Paints rows `scroll..` of a `total`-row stack into `area`, one text line
/// each, and returns how many rows fitted.
///
/// The area is cleared first: it is assembled from transient pieces with no
/// dirty flags of their own, so scrolling would otherwise smear.
pub fn rows(
    target: &mut dyn RenderTarget,
    area: Area,
    scroll: usize,
    total: usize,
    mut draw_row: impl FnMut(&mut dyn RenderTarget, usize, Area),
) -> usize {
    if area.w == 0 || area.h == 0 {
        return 0;
    }
    target.clear(area);
    let lh = target.line_height().max(1);
    let visible = (area.h / lh) as usize;
    let overflow = total > visible && area.w > BAR_W;
    let w = if overflow { area.w - BAR_W } else { area.w };

    for r in 0..visible {
        let i = scroll + r;
        if i >= total {
            break;
        }
        draw_row(target, i, Area::new(area.x, area.y + r as u16 * lh, w, lh));
    }

    if overflow {
        let mut sb = Scrollbar::new();
        sb.set(total, visible, scroll);
        sb.view(target, Area::new(area.x + area.w - 3, area.y, 3, area.h));
    }
    visible
}

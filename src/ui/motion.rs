//! Chrome motion.
//!
//! SnappyRef chrome animates a small set of task-oriented state changes:
//!
//! - active indicators sliding between positions,
//! - fast overlay entrances (opacity plus tiny settle),
//! - tactile press/acquire feedback, and
//! - short canvas pulses.
//!
//! Everything else stays still. Indicator slides go through
//! `Context::animate_value_with_time`, which stops scheduling repaints once
//! the value reaches its target. When the pointer is idle, nothing here
//! repaints.

use std::time::{Duration, Instant};

use eframe::egui;

use super::theme;

/// Animated position for an active indicator sliding between targets.
/// The first call for an id returns `target` immediately (no entrance
/// motion); later target changes ease over
/// [`theme::MOTION_INDICATOR_SECONDS`].
pub(crate) fn slide_indicator(ctx: &egui::Context, id: egui::Id, target: f32) -> f32 {
    ctx.animate_value_with_time(id, target, theme::MOTION_INDICATOR_SECONDS)
}

/// Fast overlay entrance: opacity plus tiny settle. Call every frame with the
/// surface visibility, including while hidden, so the next open starts at 0.
pub(crate) fn overlay_entrance(ctx: &egui::Context, id: egui::Id, open: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        id.with("overlay-entrance"),
        open,
        theme::MOTION_SETTLE_SECONDS,
        theme::motion_easing,
    )
}

/// Pass-scoped entrance for surfaces that are only rendered while open, such
/// as the search palette. Reappearing after a skipped pass restarts at 0.
pub(crate) fn entrance_on_appear(ctx: &egui::Context, id: egui::Id) -> f32 {
    let pass_key = id.with("entrance-pass");
    let animation_id = id.with("entrance-value");
    let now = ctx.cumulative_pass_nr();
    let last = ctx.data(|data| data.get_temp::<u64>(pass_key));
    ctx.data_mut(|data| data.insert_temp(pass_key, now));
    if last.is_none_or(|last| now.saturating_sub(last) > 1) {
        ctx.animate_bool_with_time(animation_id, false, 0.0);
    }
    ctx.animate_bool_with_time_and_easing(
        animation_id,
        true,
        theme::MOTION_SETTLE_SECONDS,
        theme::motion_easing,
    )
}

pub(crate) fn settle_offset(progress: f32) -> f32 {
    theme::MOTION_SETTLE_DISTANCE * (1.0 - progress.clamp(0.0, 1.0))
}

pub(crate) fn settle_layer_scale(
    ctx: &egui::Context,
    layer_id: egui::LayerId,
    anchor: egui::Pos2,
    progress: f32,
) {
    let progress = progress.clamp(0.0, 1.0);
    let scale = egui::lerp(theme::MOTION_ENTER_SCALE..=1.0, progress);
    let translation = anchor.to_vec2() - anchor.to_vec2() * scale;
    ctx.set_transform_layer(layer_id, egui::emath::TSTransform::new(translation, scale));
}

pub(crate) fn hover_amount(ctx: &egui::Context, id: egui::Id, hovered: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        id.with("hover"),
        hovered,
        theme::MOTION_FEEDBACK_SECONDS,
        theme::motion_easing,
    )
}

pub(crate) fn press_amount(ctx: &egui::Context, id: egui::Id, pressed: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        id.with("press"),
        pressed,
        theme::MOTION_FEEDBACK_SECONDS,
        theme::motion_easing,
    )
}

pub(crate) fn acquire_amount(ctx: &egui::Context, id: egui::Id, acquired: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        id.with("acquire"),
        acquired,
        theme::MOTION_FEEDBACK_SECONDS,
        theme::motion_easing,
    )
}

pub(crate) fn pressed_rect(rect: egui::Rect, press: f32) -> egui::Rect {
    if press <= 0.0 {
        return rect;
    }
    let scale = egui::lerp(1.0..=theme::MOTION_PRESS_SCALE, press.clamp(0.0, 1.0));
    egui::Rect::from_center_size(rect.center(), rect.size() * scale)
}

pub(crate) fn mix(from: egui::Color32, to: egui::Color32, t: f32) -> egui::Color32 {
    from.lerp_to_gamma(to, t.clamp(0.0, 1.0))
}

pub(crate) fn hover_dissolve(ctx: &egui::Context, id: egui::Id, hot: bool) -> f32 {
    let seconds = if hot {
        theme::MOTION_FEEDBACK_SECONDS
    } else {
        theme::MOTION_CONTENT_SECONDS
    };
    ctx.animate_bool_with_time_and_easing(
        id.with("hover-dissolve"),
        hot,
        seconds,
        theme::motion_easing,
    )
}

pub(crate) fn pulse_decay(ctx: &egui::Context, started_at: Option<Instant>) -> Option<f32> {
    let started_at = started_at?;
    let t = started_at.elapsed().as_secs_f32() / theme::MOTION_PULSE_SECONDS;
    if t >= 1.0 {
        return None;
    }
    ctx.request_repaint_after(Duration::from_millis(16));
    Some(1.0 - theme::motion_easing(t.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_rect_keeps_center_and_only_paints_smaller() {
        let rect = egui::Rect::from_center_size(egui::pos2(50.0, 20.0), egui::vec2(100.0, 40.0));
        let pressed = pressed_rect(rect, 1.0);
        assert_eq!(pressed.center(), rect.center());
        assert!(pressed.width() > 0.0 && pressed.width() < rect.width());
        assert!(pressed.height() > 0.0 && pressed.height() < rect.height());
        assert!((pressed.width() / pressed.height() - 2.5).abs() < 0.001);
        assert_eq!(pressed_rect(rect, 0.0), rect);
    }
}

//! Choreographed-reveal motion layer.
//!
//! SnappyRef chrome animates a small set of task-oriented state changes:
//!
//! - content cascading in, in reading order (ease-out fade + small rise,
//!   short per-row stagger), and
//! - active indicators sliding between positions.
//! - fast overlay entrances (opacity plus tiny settle),
//! - tactile press/acquire feedback, and
//! - short canvas pulses.
//!
//! Everything else stays still. Repaint discipline: cascade reveals track an
//! explicit start instant and request repaints only while unsettled;
//! indicator slides go through `Context::animate_value_with_time`, which
//! stops scheduling repaints once the value reaches its target. When all
//! reveals have settled and the pointer is idle, nothing here repaints.

use std::time::{Duration, Instant};

use eframe::egui;

use super::theme;

/// Entrance state for one cascade item. Apply `opacity` to everything the
/// item paints, and paint the item's content at `true_y + rise` so it enters
/// slightly low and settles upward into its true position (`rise` decays to
/// exactly zero). The allocated rect never moves; only painting offsets.
#[derive(Clone, Copy)]
pub(crate) struct RevealState {
    pub opacity: f32,
    pub rise: f32,
}

impl RevealState {
    pub(crate) const SETTLED: RevealState = RevealState {
        opacity: 1.0,
        rise: 0.0,
    };
}

fn cascade_start_key(list_id: egui::Id) -> egui::Id {
    list_id.with("cascade_start")
}

/// Restart the cascade for `list_id` so its items replay their entrance on
/// the next paint. Call when the surface (re)appears or its content is
/// replaced; never call every frame.
pub(crate) fn restart_cascade(ctx: &egui::Context, list_id: egui::Id) {
    ctx.data_mut(|data| data.insert_temp(cascade_start_key(list_id), Instant::now()));
}

/// Restart the cascade when `fingerprint` differs from the one recorded for
/// `list_id`. Use for surfaces whose content is replaced in place (for
/// example a result grid after a new query).
#[allow(dead_code)]
pub(crate) fn restart_cascade_on_change(
    ctx: &egui::Context,
    list_id: egui::Id,
    fingerprint: egui::Id,
) {
    let key = list_id.with("cascade_fingerprint");
    let previous = ctx.data(|data| data.get_temp::<egui::Id>(key));
    if previous != Some(fingerprint) {
        ctx.data_mut(|data| data.insert_temp(key, fingerprint));
        restart_cascade(ctx, list_id);
    }
}

/// Entrance progress for item `index` of cascade `list_id`. Items fade and
/// rise in reading order with [`theme::MOTION_REVEAL_STAGGER_SECONDS`]
/// offsets, capped at [`theme::MOTION_REVEAL_MAX_STAGGER_STEPS`] steps. If
/// the cascade was never started the item renders settled: content that was
/// already on screen must not animate.
pub(crate) fn cascade_reveal(ctx: &egui::Context, list_id: egui::Id, index: usize) -> RevealState {
    let Some(start) = ctx.data(|data| data.get_temp::<Instant>(cascade_start_key(list_id))) else {
        return RevealState::SETTLED;
    };
    let step = index.min(theme::MOTION_REVEAL_MAX_STAGGER_STEPS);
    let delay = step as f32 * theme::MOTION_REVEAL_STAGGER_SECONDS;
    let elapsed = start.elapsed().as_secs_f32();
    let progress = ((elapsed - delay) / theme::MOTION_REVEAL_SECONDS).clamp(0.0, 1.0);
    if progress < 1.0 {
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    let eased = ease_out_cubic(progress);
    RevealState {
        opacity: eased,
        rise: (1.0 - eased) * theme::MOTION_REVEAL_RISE,
    }
}

/// Cascade reveal that restarts automatically whenever the surface
/// reappears after not being painted (sidebar opened, tab switched back,
/// menu/toolbar/dialog shown). Safe to call once per item per frame.
pub(crate) fn cascade_reveal_on_appear(
    ctx: &egui::Context,
    list_id: egui::Id,
    index: usize,
) -> RevealState {
    #[cfg(test)]
    {
        let _ = (ctx, list_id, index);
        return RevealState::SETTLED;
    }

    #[cfg(not(test))]
    {
        let pass_key = list_id.with("cascade_last_pass");
        let pass = ctx.cumulative_pass_nr();
        let last_painted = ctx.data(|data| data.get_temp::<u64>(pass_key));
        if last_painted.is_none_or(|last| pass.saturating_sub(last) > 2) {
            restart_cascade(ctx, list_id);
        }
        ctx.data_mut(|data| data.insert_temp(pass_key, pass));
        cascade_reveal(ctx, list_id, index)
    }
}

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
        egui::emath::easing::cubic_out,
    )
}

pub(crate) fn press_amount(ctx: &egui::Context, id: egui::Id, pressed: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        id.with("press"),
        pressed,
        theme::MOTION_FEEDBACK_SECONDS,
        egui::emath::easing::cubic_out,
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

pub(crate) fn pulse_decay(
    ctx: &egui::Context,
    _id: egui::Id,
    started_at: Option<Instant>,
) -> Option<f32> {
    let started_at = started_at?;
    let t = started_at.elapsed().as_secs_f32() / theme::MOTION_PULSE_SECONDS;
    if t >= 1.0 {
        return None;
    }
    ctx.request_repaint_after(Duration::from_millis(16));
    Some(1.0 - theme::motion_easing(t.clamp(0.0, 1.0)))
}

fn ease_out_cubic(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_without_start_renders_settled() {
        let ctx = egui::Context::default();
        let state = cascade_reveal(&ctx, egui::Id::new("never-started"), 0);
        assert_eq!(state.opacity, 1.0);
        assert_eq!(state.rise, 0.0);
    }

    #[test]
    fn restarted_cascade_begins_hidden_and_staggers_later_rows() {
        let ctx = egui::Context::default();
        let list = egui::Id::new("list");
        restart_cascade(&ctx, list);
        let first = cascade_reveal(&ctx, list, 0);
        let later = cascade_reveal(&ctx, list, 4);
        assert!(first.opacity < 1.0);
        assert!(later.opacity <= first.opacity);
        assert!(later.rise >= first.rise);
    }

    #[test]
    fn stagger_steps_are_capped_for_long_lists() {
        let ctx = egui::Context::default();
        let list = egui::Id::new("long-list");
        restart_cascade(&ctx, list);
        let capped = cascade_reveal(&ctx, list, theme::MOTION_REVEAL_MAX_STAGGER_STEPS);
        let beyond = cascade_reveal(&ctx, list, theme::MOTION_REVEAL_MAX_STAGGER_STEPS + 40);
        assert_eq!(capped.opacity, beyond.opacity);
    }

    #[test]
    fn ease_out_cubic_settles_at_one() {
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert!(ease_out_cubic(0.5) > 0.5);
    }

    #[test]
    fn pressed_rect_keeps_center_and_only_paints_smaller() {
        let rect = egui::Rect::from_center_size(egui::pos2(50.0, 20.0), egui::vec2(100.0, 40.0));
        let pressed = pressed_rect(rect, 1.0);
        assert_eq!(pressed.center(), rect.center());
        assert!(pressed.width() < rect.width());
    }
}

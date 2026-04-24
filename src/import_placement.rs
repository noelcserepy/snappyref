use eframe::egui;

const IMPORT_SNAP_MAX_DISTANCE_SCREEN: f32 = 32.0;

pub(crate) fn reserve_import_world(
    layout: &mut Vec<crate::data::BoardLayoutItem>,
    desired_world: egui::Pos2,
    size: egui::Vec2,
    gap: f32,
    snap_distance_world: f32,
) -> egui::Pos2 {
    let reserved_world = snap_import_world(layout, desired_world, size, gap, snap_distance_world)
        .unwrap_or(desired_world);
    layout.push(crate::data::BoardLayoutItem {
        id: -((layout.len() as i64) + 1),
        x: reserved_world.x,
        y: reserved_world.y,
        width: size.x,
        height: size.y,
    });
    reserved_world
}

pub(crate) fn import_snap_distance_world(canvas: &crate::canvas::CanvasState) -> f32 {
    canvas.screen_distance_to_world(IMPORT_SNAP_MAX_DISTANCE_SCREEN)
}

fn snap_import_world(
    layout: &[crate::data::BoardLayoutItem],
    desired_world: egui::Pos2,
    size: egui::Vec2,
    gap: f32,
    snap_distance_world: f32,
) -> Option<egui::Pos2> {
    let desired_rect = egui::Rect::from_center_size(desired_world, size);
    let desired_overlaps_pending_placeholder =
        layout.iter().filter(|item| item.id < 0).any(|item| {
            rects_overlap_with_gap(
                desired_rect,
                egui::Rect::from_center_size(
                    egui::pos2(item.x, item.y),
                    egui::vec2(item.width, item.height),
                ),
                gap,
            )
        });
    let obstacles = layout
        .iter()
        .map(|item| {
            crate::layout_snap::SnapObstacle::from_center_size(
                egui::pos2(item.x, item.y),
                egui::vec2(item.width, item.height),
            )
        })
        .collect::<Vec<_>>();
    if obstacles.is_empty() {
        return None;
    }

    let effective_snap_distance = if desired_overlaps_pending_placeholder {
        snap_distance_world.max(size.x.max(size.y) + gap)
    } else {
        snap_distance_world
    };
    let request = crate::layout_snap::MoveSnapRequest {
        desired_rect,
        drag_delta: egui::Vec2::ZERO,
        gap,
        max_snap_distance: effective_snap_distance,
    };
    let candidate = crate::layout_snap::best_move_snap_candidate(&obstacles, &request)?;
    (desired_overlaps_pending_placeholder
        || candidate.rect.center().distance(desired_world) <= snap_distance_world)
        .then_some(candidate.rect.center())
}

fn rects_overlap_with_gap(left: egui::Rect, right: egui::Rect, gap: f32) -> bool {
    left.left() < right.right() + gap
        && left.right() > right.left() - gap
        && left.top() < right.bottom() + gap
        && left.bottom() > right.top() - gap
}

#[cfg(test)]
mod tests {
    use super::{IMPORT_SNAP_MAX_DISTANCE_SCREEN, reserve_import_world};

    #[test]
    fn import_snap_uses_move_snap_solver_near_existing_item() {
        let mut layout = vec![crate::data::BoardLayoutItem {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }];
        let reserved = reserve_import_world(
            &mut layout,
            eframe::egui::pos2(126.0, 0.0),
            eframe::egui::vec2(100.0, 100.0),
            24.0,
            IMPORT_SNAP_MAX_DISTANCE_SCREEN,
        );

        assert_eq!(reserved, eframe::egui::pos2(124.0, 0.0));
    }

    #[test]
    fn import_snap_stays_at_pointer_when_no_target_is_nearby() {
        let mut layout = vec![crate::data::BoardLayoutItem {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }];
        let desired = eframe::egui::pos2(300.0, 0.0);
        let reserved = reserve_import_world(
            &mut layout,
            desired,
            eframe::egui::vec2(100.0, 100.0),
            24.0,
            IMPORT_SNAP_MAX_DISTANCE_SCREEN,
        );

        assert_eq!(reserved, desired);
    }

    #[test]
    fn import_snap_escapes_overlapping_pending_placeholder() {
        let mut layout = vec![crate::data::BoardLayoutItem {
            id: -1,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }];
        let reserved = reserve_import_world(
            &mut layout,
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(100.0, 100.0),
            24.0,
            IMPORT_SNAP_MAX_DISTANCE_SCREEN,
        );

        assert_eq!(reserved, eframe::egui::pos2(124.0, 0.0));
    }

    #[test]
    fn import_snap_does_not_escape_real_image_overlap_when_not_near_snap_target() {
        let mut layout = vec![crate::data::BoardLayoutItem {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }];
        let desired = eframe::egui::Pos2::ZERO;
        let reserved = reserve_import_world(
            &mut layout,
            desired,
            eframe::egui::vec2(100.0, 100.0),
            24.0,
            IMPORT_SNAP_MAX_DISTANCE_SCREEN,
        );

        assert_eq!(reserved, desired);
    }
}

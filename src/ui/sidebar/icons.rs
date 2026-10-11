//! The sidebar's glyphs, drawn on a 16 unit grid at 14 points like the
//! Group card's.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Mouse,
    Trackpad,
    NoGuide,
    Grid,
    Dots,
    Rotation,
    Gap,
    OnTop,
    Ask,
    FullAccess,
    General,
    Keys,
}

pub(super) fn paint(p: &egui::Painter, center: Pos2, icon: Icon, color: Color32) {
    const SIZE: f32 = 14.0;
    let origin = center - Vec2::splat(SIZE * 0.5);
    let scale = SIZE / 16.0;
    let point = |x: f32, y: f32| origin + egui::vec2(x, y) * scale;
    let stroke = Stroke::new(1.25, color);
    let line = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    let frame = |x: f32, y: f32, w: f32, h: f32, radius: f32| {
        p.rect_stroke(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            radius * scale,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    let dot = |x: f32, y: f32, radius: f32| p.circle_filled(point(x, y), radius * scale, color);
    match icon {
        Icon::Mouse => {
            frame(4.5, 1.5, 7.0, 13.0, 3.5);
            line(&[(8.0, 4.0), (8.0, 6.5)]);
        }
        Icon::Trackpad => {
            frame(1.5, 3.0, 13.0, 10.0, 2.0);
            line(&[(1.5, 10.0), (14.5, 10.0)]);
            line(&[(8.0, 10.0), (8.0, 13.0)]);
        }
        Icon::NoGuide => {
            frame(2.0, 2.0, 12.0, 12.0, 2.0);
            line(&[(4.5, 11.5), (11.5, 4.5)]);
        }
        Icon::Grid => {
            frame(2.0, 2.0, 12.0, 12.0, 2.0);
            for at in [6.0, 10.0] {
                line(&[(at, 2.0), (at, 14.0)]);
                line(&[(2.0, at), (14.0, at)]);
            }
        }
        Icon::Dots => {
            for x in [4.0, 8.0, 12.0] {
                for y in [4.0, 8.0, 12.0] {
                    dot(x, y, 1.2);
                }
            }
        }
        Icon::Rotation => {
            let arc: Vec<_> = (0..=20)
                .map(|i| {
                    let angle = (160.0 + 240.0 * i as f32 / 20.0_f32).to_radians();
                    point(8.0 + 5.5 * angle.cos(), 8.5 + 5.5 * angle.sin())
                })
                .collect();
            p.add(egui::Shape::line(arc, stroke));
            line(&[(11.2, 1.8), (12.2, 4.6), (9.3, 5.4)]);
        }
        Icon::Gap => {
            line(&[(2.0, 8.0), (14.0, 8.0)]);
            line(&[(5.0, 5.0), (2.0, 8.0), (5.0, 11.0)]);
            line(&[(11.0, 5.0), (14.0, 8.0), (11.0, 11.0)]);
        }
        Icon::OnTop => {
            frame(1.5, 5.5, 8.5, 8.5, 1.5);
            p.rect_filled(
                Rect::from_min_max(point(5.5, 1.5), point(14.5, 10.5)),
                1.5 * scale,
                color,
            );
        }
        Icon::Ask => {
            frame(1.5, 2.0, 13.0, 9.5, 2.5);
            line(&[(5.0, 11.5), (5.0, 14.5), (8.5, 11.5)]);
            for x in [5.0, 8.0, 11.0] {
                dot(x, 6.8, 0.9);
            }
        }
        Icon::FullAccess => {
            line(&[
                (9.5, 1.5),
                (4.0, 9.0),
                (8.0, 9.0),
                (6.5, 14.5),
                (12.0, 7.0),
                (8.0, 7.0),
                (9.5, 1.5),
            ]);
        }
        Icon::General => {
            line(&[(2.0, 5.0), (14.0, 5.0)]);
            line(&[(2.0, 11.0), (14.0, 11.0)]);
            dot(10.0, 5.0, 2.0);
            dot(6.0, 11.0, 2.0);
        }
        Icon::Keys => {
            frame(1.0, 3.5, 14.0, 9.0, 2.0);
            for x in [4.0, 6.7, 9.3, 12.0] {
                dot(x, 6.5, 0.8);
            }
            line(&[(5.0, 9.8), (11.0, 9.8)]);
        }
    }
}

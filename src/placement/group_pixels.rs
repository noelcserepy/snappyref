//! Pixel observations used by the grouping core and by canvas placement.
use crate::data::{Group, RectGeometry};
use crate::grouping::{self, Board, Geometry, Id};
use eframe::egui::{self, Pos2, Vec2};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Pixels {
    pub rects: BTreeMap<Id, RectGeometry>,
    pub intake: BTreeMap<Id, grouping::Member>,
}
pub(crate) struct Layout {
    pub offsets: BTreeMap<Id, Vec2>,
    pub sizes: BTreeMap<Id, Vec2>,
    pub content: Vec2,
    pub min: Vec2,
}
fn alignment(n: usize, space: f32) -> f32 {
    space.max(0.0) * n as f32 * 0.5
}
impl Pixels {
    pub fn layout(&self, b: &Board, g: Id) -> Layout {
        let group = &b.groups[&g];
        let mut result = Layout {
            offsets: BTreeMap::new(),
            sizes: BTreeMap::new(),
            content: Vec2::ZERO,
            min: Vec2::ZERO,
        };
        let rows = b.rows(g);
        let primary = group.primary();
        let cross = 1 - primary;
        if group.flow == "Free" {
            for &m in &group.order {
                result.sizes.insert(m, Vec2::from(b.members[&m].size));
            }
            let bounds = b
                .flowing(g)
                .iter()
                .filter_map(|m| self.rects.get(m))
                .map(|r| r.visual_bounds())
                .reduce(|a, b| a.union(b));
            result.min = bounds.map_or(Vec2::ZERO, |bounds| bounds.min.to_vec2().min(Vec2::ZERO));
            result.content = bounds.map_or(Vec2::from(group.size), |r| r.size());
            return result;
        }
        let mut primary_cells = BTreeMap::new();
        let mut row_heights = Vec::new();
        let mut row_fills = Vec::new();
        for row in &rows {
            let (space, count) = b.allocation(g, row, primary);
            let mut height: f32 = 0.0;
            for &m in row {
                let member = &b.members[&m];
                let mut size = Vec2::from(member.size);
                if member.fill.contains(&primary) {
                    let dimension = if count == 0 {
                        0.0
                    } else {
                        space / count as f32
                    };
                    primary_cells.insert(m, dimension);
                    if member.kind == "Image" && size[primary] > 0.0 {
                        size *= dimension / size[primary];
                    } else {
                        size[primary] = dimension;
                    }
                }
                if !member.fill.contains(&cross) {
                    height = height.max(size[cross]);
                }
                result.sizes.insert(m, size);
            }
            row_heights.push(height);
            row_fills.push(row.iter().any(|m| b.members[m].fill.contains(&cross)));
        }
        let row_gaps = group.rowGap * rows.len().saturating_sub(1) as f32;
        let fill_rows = row_fills.iter().filter(|&&fill| fill).count();
        let remaining = (group.size[cross] - row_heights.iter().sum::<f32>() - row_gaps).max(0.0);
        for (i, height) in row_heights.iter_mut().enumerate() {
            if group.fixed.contains(&cross) {
                if rows.len() == 1 {
                    *height = group.size[cross];
                } else if row_fills[i] && fill_rows > 0 {
                    *height += remaining / fill_rows as f32;
                }
            }
        }
        let stack_size = row_heights.iter().sum::<f32>() + row_gaps;
        let mut cross_pos = if group.fixed.contains(&cross) {
            alignment(group.align[cross], group.size[cross] - stack_size)
        } else {
            0.0
        };
        let mut bounds: Option<egui::Rect> = None;
        for (r, row) in rows.iter().enumerate() {
            for &m in row {
                let member = &b.members[&m];
                let mut size = result.sizes[&m];
                if member.fill.contains(&cross) {
                    if member.kind == "Image" {
                        let original = Vec2::from(member.size);
                        if original[cross] > 0.0 {
                            let cross_factor = row_heights[r] / original[cross];
                            let factor =
                                if member.fill.contains(&primary) && original[primary] > 0.0 {
                                    cross_factor.min(size[primary] / original[primary])
                                } else {
                                    cross_factor
                                };
                            size = original * factor;
                        }
                    } else {
                        size[cross] = row_heights[r];
                    }
                }
                result.sizes.insert(m, size);
            }
            // A ratio-fitted image still owns its allocated Fill cell. Auto only
            // distributes space left after those cells, not unused image pixels.
            let cell_size = |m: &Id| {
                primary_cells
                    .get(m)
                    .copied()
                    .unwrap_or(result.sizes[m][primary])
            };
            let used = row.iter().map(cell_size).sum::<f32>();
            let n = row.len() as f32;
            let leftover = (group.size[primary] - used).max(0.0);
            let (gap, mut position) = match group.gap.as_str() {
                "Between" => (
                    if row.len() > 1 {
                        leftover / (n - 1.0)
                    } else {
                        0.0
                    },
                    0.0,
                ),
                "Evenly" => (leftover / (n + 1.0), leftover / (n + 1.0)),
                "Around" => (leftover / n, leftover / n * 0.5),
                _ => (
                    group.number,
                    if group.fixed.contains(&primary) {
                        alignment(
                            group.align[primary],
                            group.size[primary] - used - group.number * (n - 1.0),
                        )
                    } else {
                        0.0
                    },
                ),
            };
            for &m in row {
                let size = result.sizes[&m];
                let mut offset = Vec2::ZERO;
                let cell = cell_size(&m);
                offset[primary] = position
                    + alignment(
                        if group.gap == "Number" {
                            group.align[primary]
                        } else {
                            0
                        },
                        cell - size[primary],
                    );
                offset[cross] =
                    cross_pos + alignment(group.align[cross], row_heights[r] - size[cross]);
                result.offsets.insert(m, offset);
                let rect = egui::Rect::from_min_size(Pos2::ZERO + offset, size);
                bounds = Some(bounds.map_or(rect, |b| b.union(rect)));
                position += cell + gap;
            }
            cross_pos += row_heights[r] + group.rowGap;
        }
        result.min = bounds.map_or(Vec2::ZERO, |bounds| bounds.min.to_vec2().min(Vec2::ZERO));
        result.content = bounds.map_or(Vec2::from(group.size), |b| {
            b.max.to_vec2().max(Vec2::ZERO) - b.min.to_vec2().min(Vec2::ZERO)
        });
        for &m in &group.order {
            if b.members[&m].absolute {
                result.sizes.insert(m, Vec2::from(b.members[&m].size));
            }
        }
        result
    }
}
impl Geometry for Pixels {
    fn pasted_member(&self, id: Id, width: f32) -> grouping::Member {
        self.intake
            .get(&id)
            .cloned()
            .unwrap_or_else(|| grouping::Member::fixed("Image", [width; 2]))
    }
    fn fit(&self, member: &grouping::Member, width: f32) -> [f32; 2] {
        let factor = if member.size[0] > width {
            width / member.size[0]
        } else {
            1.0
        };
        [member.size[0] * factor, member.size[1] * factor]
    }
    fn rendered(&self, b: &Board, g: Id, m: Id, a: usize) -> f32 {
        self.layout(b, g).sizes[&m][a]
    }
    fn measured(&self, b: &Board, g: Id, a: usize) -> f32 {
        if b.groups[&g].fixed.contains(&a) || b.flowing(g).is_empty() {
            b.groups[&g].size[a]
        } else {
            self.layout(b, g).content[a].max(1.0)
        }
    }
}
pub(crate) fn config(group: &Group) -> grouping::Group {
    grouping::Group {
        order: group.members.clone(),
        flow: group.flow.as_str().into(),
        fixed: (0..2).filter(|&a| group.layout.fixed[a]).collect(),
        size: [
            (group.container.scaled_size().x - group.padding * 2.0).max(0.0),
            (group.container.scaled_size().y - group.padding * 2.0).max(0.0),
        ],
        gap: group.layout.auto.clone(),
        number: group.gap,
        rowGap: group.layout.row_gap,
        padding: group.padding,
        align: group.layout.align,
        clip: group.layout.clip,
        name: group.name.clone(),
    }
}

#[cfg(test)]
mod dock_geometry_examples {
    use super::*;
    use std::collections::BTreeSet;

    fn board(sizes: &[[f32; 2]], kind: &str) -> Board {
        let mut group = grouping::Group::new(0, 300.0, 10.0);
        group.flow = "Row".into();
        group.fixed = BTreeSet::from([0, 1]);
        group.size = [300.0, 100.0];
        group.order = (0..sizes.len() as i64).collect();
        Board {
            groups: BTreeMap::from([(0, group)]),
            live: (0..sizes.len() as i64).collect(),
            members: sizes
                .iter()
                .enumerate()
                .map(|(i, &size)| (i as i64, grouping::Member::fixed(kind, size)))
                .collect(),
        }
    }
    #[test]
    fn grouping_numeric_fill_and_overflow_examples() {
        let mut b = board(
            &[[100.0, 20.0], [80.0, 20.0], [20.0, 20.0], [20.0, 20.0]],
            "Textbox",
        );
        for m in [2, 3] {
            b.members.get_mut(&m).unwrap().fill.insert(0);
        }
        let p = Pixels::default();
        let layout = p.layout(&b, 0);
        assert_eq!(layout.sizes[&2].x, 45.0);
        assert_eq!(layout.sizes[&3].x, 45.0);
        b.groups.get_mut(&0).unwrap().size[0] = 180.0;
        let layout = p.layout(&b, 0);
        assert_eq!(layout.sizes[&2].x, 0.0);
        assert_eq!(layout.sizes[&0].x, 100.0);
    }
    #[test]
    fn grouping_image_aspect_fit_example() {
        let mut b = board(&[[400.0, 200.0]], "Image");
        let g = b.groups.get_mut(&0).unwrap();
        g.size = [100.0, 100.0];
        g.align = [1, 1];
        b.members.get_mut(&0).unwrap().fill = BTreeSet::from([0, 1]);
        let layout = Pixels::default().layout(&b, 0);
        assert_eq!(layout.sizes[&0], egui::vec2(100.0, 50.0));
        assert_eq!(layout.offsets[&0], egui::vec2(0.0, 25.0));
    }
    #[test]
    fn grouping_auto_spacing_examples() {
        let mut b = board(&[[40.0, 20.0]; 3], "Textbox");
        for (mode, start, gap) in [
            ("Between", 0.0, 90.0),
            ("Evenly", 45.0, 45.0),
            ("Around", 30.0, 60.0),
        ] {
            b.groups.get_mut(&0).unwrap().gap = mode.into();
            let layout = Pixels::default().layout(&b, 0);
            for m in 0..3 {
                assert_eq!(layout.offsets[&m].x, start + m as f32 * (40.0 + gap));
            }
        }
    }
    #[test]
    fn grouping_outer_size_excludes_padding() {
        let group = Group {
            id: 0,
            name: "Faces".into(),
            members: vec![],
            container: RectGeometry::from_bounds(
                egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)),
                0,
            ),
            flow: crate::data::GroupFlow::Wrap,
            gap: 24.0,
            padding: 16.0,
            layout: Default::default(),
        };
        assert_eq!(config(&group).size, [68.0, 68.0]);
    }
}

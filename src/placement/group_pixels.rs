//! Pixel observations used by the grouping core and by canvas placement.
use crate::data::{Group, RectGeometry};
use crate::grouping::{self, Board, Geometry, Id};
use eframe::egui::{self, Pos2, Vec2};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Pixels {
    pub rects: BTreeMap<Id, RectGeometry>,
    pub intake: BTreeMap<Id, grouping::Member>,
    /// Intrinsic Asset bounds at the current rotation, used to recover image ratios.
    pub image_sizes: BTreeMap<Id, Vec2>,
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
    // Asset dimensions supply a ratio only while solving active Fill. They never
    // replace the displayed sizes stored by the edit core or frozen by conversions.
    fn fill_inputs<'a>(&self, b: &'a Board, g: Id) -> std::borrow::Cow<'a, Board> {
        let mut inputs = std::borrow::Cow::Borrowed(b);
        if b.groups[&g].flow != "Free" {
            for &m in &b.groups[&g].order {
                let member = &b.members[&m];
                if !member.absolute
                    && !member.fill.is_empty()
                    && member.size.contains(&0.0)
                    && let Some(size) = self.image_sizes.get(&m)
                {
                    inputs.to_mut().members.get_mut(&m).unwrap().size = (*size).into();
                }
            }
        }
        inputs
    }
    pub fn layout(&self, board: &Board, g: Id) -> Layout {
        let inputs = self.fill_inputs(board, g);
        let (_, heights) = self.solve(&reserve_cross_fill(&inputs, g, None), g);
        self.solve(&reserve_cross_fill(&inputs, g, Some(&heights)), g)
            .0
    }
    /// Returns the layout and its row heights.
    fn solve(&self, b: &Board, g: Id) -> (Layout, Vec<f32>) {
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
            result.content = bounds.map_or(Vec2::new(group.capacity(0), group.capacity(1)), |r| {
                r.size()
            });
            return (result, Vec::new());
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
        let remaining =
            (group.capacity(cross) - row_heights.iter().sum::<f32>() - row_gaps).max(0.0);
        for (i, height) in row_heights.iter_mut().enumerate() {
            if group.fixed.contains(&cross) {
                if rows.len() == 1 {
                    *height = group.capacity(cross);
                } else if row_fills[i] && fill_rows > 0 {
                    *height += remaining / fill_rows as f32;
                }
            }
        }
        let stack_size = row_heights.iter().sum::<f32>() + row_gaps;
        let mut cross_pos = if group.fixed.contains(&cross) {
            alignment(group.align[cross], group.capacity(cross) - stack_size)
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
                            // The image fits the width its row reserved or allocated.
                            let factor = if original[primary] > 0.0 {
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
            let leftover = (group.capacity(primary) - used).max(0.0);
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
                            group.capacity(primary) - used - group.number * (n - 1.0),
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
        result.content = bounds.map_or(Vec2::new(group.capacity(0), group.capacity(1)), |b| {
            b.max.to_vec2().max(Vec2::ZERO) - b.min.to_vec2().min(Vec2::ZERO)
        });
        for &m in &group.order {
            if b.members[&m].absolute {
                result.sizes.insert(m, Vec2::from(b.members[&m].size));
            }
        }
        (result, row_heights)
    }
}
/// A cross-Fill image's primary extent follows its row height, so it can't come
/// from the last render. The first pass reserves nothing to solve row heights. The
/// second reserves each image's extent at its row height. Fill needs a Fixed axis,
/// so a single row's height is exact.
fn reserve_cross_fill<'a>(
    b: &'a Board,
    g: Id,
    heights: Option<&[f32]>,
) -> std::borrow::Cow<'a, Board> {
    let mut inputs = std::borrow::Cow::Borrowed(b);
    let group = &b.groups[&g];
    if group.flow == "Free" {
        return inputs;
    }
    let primary = group.primary();
    let cross = 1 - primary;
    for (r, row) in b.rows(g).iter().enumerate() {
        for m in row {
            let member = &b.members[m];
            if member.kind == "Image"
                && member.fill.contains(&cross)
                && !member.fill.contains(&primary)
            {
                let size = &mut inputs.to_mut().members.get_mut(m).unwrap().size;
                let factor = match heights {
                    Some(heights) if size[cross] > 0.0 => heights[r] / size[cross],
                    Some(_) => 1.0,
                    None => 0.0,
                };
                *size = size.map(|x| x * factor);
            }
        }
    }
    inputs
}
impl Geometry for Pixels {
    fn wrap_arrow_target(&self, b: &Board, g: Id, m: Id, direction: &str) -> Option<Id> {
        let inputs = self.fill_inputs(b, g);
        let rows = inputs.rows(g);
        let row = rows.iter().position(|row| row.contains(&m))?;
        let column = rows[row].iter().position(|&member| member == m)?;
        let adjacent = if direction == "Up" {
            row.checked_sub(1)?
        } else {
            row + 1
        };
        rows.get(adjacent)?.get(column).copied()
    }

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
            self.layout(b, g).content[a].max(1.0) + b.groups[&g].padding * 2.0
        }
    }
}
pub(crate) fn config(group: &Group) -> grouping::Group {
    grouping::Group {
        order: group.members.clone(),
        flow: group.flow.as_str().into(),
        fixed: (0..2).filter(|&a| group.layout.fixed[a]).collect(),
        size: group.container.scaled_size().into(),
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
        group.size = [320.0, 120.0];
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
        b.groups.get_mut(&0).unwrap().size[0] = 200.0;
        let layout = p.layout(&b, 0);
        assert_eq!(layout.sizes[&2].x, 0.0);
        assert_eq!(layout.sizes[&0].x, 100.0);
    }
    #[test]
    fn grouping_image_aspect_fit_example() {
        let mut b = board(&[[400.0, 200.0]], "Image");
        let g = b.groups.get_mut(&0).unwrap();
        g.size = [120.0, 120.0];
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
    fn grouping_outer_size_and_usable_capacity() {
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
        assert_eq!(config(&group).size, [100.0, 100.0]);
        assert_eq!(config(&group).capacity(0), 68.0);
    }
}

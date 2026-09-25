#[cfg(test)]
use super::connection::Library;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::params;

use crate::data::{Group, GroupFlow, RectGeometry};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredGroup {
    id: i64,
    name: String,
    members: Vec<i64>,
    center: [f32; 2],
    size: [f32; 2],
    scale: f32,
    rotation: f32,
    z_index: i32,
    flow: GroupFlow,
    spacing: f32,
    padding: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout: Option<crate::data::GroupLayoutSettings>,
}

impl From<&Group> for StoredGroup {
    fn from(group: &Group) -> Self {
        Self {
            id: group.id,
            name: group.name.clone(),
            members: group.members.clone(),
            center: [group.container.center.x, group.container.center.y],
            size: [group.container.size.x, group.container.size.y],
            scale: group.container.scale,
            rotation: group.container.rotation,
            z_index: group.container.z_index,
            flow: group.flow,
            spacing: group.gap,
            padding: group.padding,
            layout: Some(group.layout.clone()),
        }
    }
}

impl From<StoredGroup> for Group {
    fn from(group: StoredGroup) -> Self {
        Self {
            id: group.id,
            name: group.name,
            members: group.members,
            container: RectGeometry::new(
                eframe::egui::pos2(group.center[0], group.center[1]),
                eframe::egui::vec2(group.size[0], group.size[1]),
                group.scale,
                group.rotation,
                group.z_index,
            ),
            flow: group.flow,
            gap: group.spacing,
            padding: group.padding,
            layout: group
                .layout
                .unwrap_or_else(|| crate::data::GroupLayoutSettings {
                    row_gap: group.spacing,
                    ..Default::default()
                }),
        }
    }
}

pub(super) fn load_groups_for_board_id(
    connection: &Connection,
    board_id: i64,
) -> Result<Vec<Group>, String> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload FROM board_groups WHERE board_id = ?1",
            params![board_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("failed to load Groups: {error}"))?;
    payload
        .map(|payload| {
            serde_json::from_str::<Vec<StoredGroup>>(&payload)
                .map(|groups| groups.into_iter().map(Group::from).collect())
                .map_err(|error| format!("failed to decode Groups: {error}"))
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

pub(super) fn replace_board_groups(
    transaction: &Transaction<'_>,
    board_id: i64,
    groups: &[Group],
) -> Result<(), String> {
    let payload = serde_json::to_string(&groups.iter().map(StoredGroup::from).collect::<Vec<_>>())
        .map_err(|error| format!("failed to encode Groups: {error}"))?;
    transaction.execute(
        "INSERT INTO board_groups (board_id, payload) VALUES (?1, ?2) ON CONFLICT(board_id) DO UPDATE SET payload = excluded.payload",
        params![board_id, payload],
    ).map_err(|error| format!("failed to save Groups: {error}"))?;
    Ok(())
}

pub(super) fn prune_archived_group_members(
    transaction: &Transaction<'_>,
    board_ids: &std::collections::HashSet<i64>,
) -> Result<(), String> {
    for &board_id in board_ids {
        let live_ids = transaction
            .prepare("SELECT id FROM board_items WHERE board_id = ?1 AND archived = 0")
            .map_err(|error| error.to_string())?
            .query_map([board_id], |row| row.get::<_, i64>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<std::collections::HashSet<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut groups = load_groups_for_board_id(transaction, board_id)?;
        let mut changed = false;
        for group in &mut groups {
            let old_len = group.members.len();
            group.members.retain(|id| live_ids.contains(id));
            group
                .layout
                .members
                .retain(|id, _| group.members.contains(id));
            changed |= old_len != group.members.len();
        }
        if changed {
            replace_board_groups(transaction, board_id, &groups)?;
        }
    }
    Ok(())
}

#[cfg(test)]
impl Library {
    pub fn load_groups(&self) -> Result<Vec<Group>, String> {
        let connection = self.connection()?;
        load_groups_for_board_id(&connection, super::boards::active_board_id(&connection)?)
    }
}

#[cfg(test)]
mod tests {
    use super::{Group, GroupFlow, Library, RectGeometry};
    use crate::library::storage::BoardDurabilityOperation;

    use crate::active_board::history::{DocumentHistory, HistoryAction, HistoryChange};
    use crate::canvas::CanvasDocumentChanges;

    #[test]
    fn groups_and_history_survive_restart_and_failed_write_rolls_back_both() {
        let root = crate::test_support::TempRoot::new("snappyref-group-durability");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let group = Group {
            id: 7,
            name: "References".into(),
            members: Vec::new(),
            container: RectGeometry::new(
                eframe::egui::pos2(12.0, 34.0),
                eframe::egui::vec2(200.0, 100.0),
                1.0,
                0.0,
                0,
            ),
            flow: GroupFlow::Wrap,
            gap: 8.0,
            layout: crate::data::GroupLayoutSettings {
                row_gap: 8.0,
                ..Default::default()
            },
            padding: 12.0,
        };
        let mut history = DocumentHistory::default();
        history.record_applied(
            1,
            HistoryAction::document("Create group".into(), vec![], vec![], None).with_groups(Some(
                HistoryChange {
                    before: Some(vec![]),
                    after: Some(vec![group.clone()]),
                },
            )),
        );
        let operation = |groups| BoardDurabilityOperation::Document {
            board_id: 1,
            changes: CanvasDocumentChanges {
                groups: Some(groups),
                ..Default::default()
            },
        };
        library
            .persist_board_unit(&operation(vec![group.clone()]), &history)
            .unwrap();
        let reopened = Library::initialize(root.to_path_buf()).unwrap();
        assert_eq!(reopened.load_groups().unwrap(), vec![group.clone()]);
        let mut restored_history = reopened.load_document_history().unwrap();
        let restored_action = restored_history.undo(1).unwrap();
        let crate::active_board::history::HistoryAction::Document(action) = restored_action.action
        else {
            panic!("expected document history")
        };
        assert_eq!(action.groups.as_ref().unwrap().before, Some(vec![]));
        assert_eq!(
            action.groups.as_ref().unwrap().after,
            Some(vec![group.clone()])
        );
        assert!(restored_history.redo(1).is_some());
        reopened.connection().unwrap().execute_batch("CREATE TRIGGER reject_history BEFORE UPDATE ON board_history BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(
            reopened
                .persist_board_unit(&operation(vec![]), &restored_history)
                .is_err()
        );
        assert_eq!(reopened.load_groups().unwrap(), vec![group.clone()]);
        assert_eq!(
            reopened
                .load_document_history()
                .unwrap()
                .undo(1)
                .unwrap()
                .action
                .label(),
            history.undo(1).unwrap().action.label()
        );
    }
}

#[cfg(test)]
mod archive_tests {
    use super::*;
    use crate::data::{Placement, PlacementState};
    use eframe::egui;

    #[test]
    fn library_remove_multiple_assets_prunes_groups_on_every_board_after_reopen() {
        let root = crate::test_support::TempRoot::new("library-remove-groups");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let second = library.fixture_board("Second").unwrap();
        let assets: Vec<_> = (0..3)
            .map(|i| {
                let path = library
                    .store_rgba_image(&format!("asset-{i}"), &[i, 40, 90, 255], 1, 1)
                    .unwrap();
                library
                    .register_original(&path, "image/png", 1, 1, 4, None)
                    .unwrap()
            })
            .collect();
        for board in [1, second.id] {
            let mut snapshot = library.load_board_document(board).unwrap();
            snapshot.placements = assets
                .iter()
                .map(|asset| {
                    Placement::loaded(
                        PlacementState::new(
                            library.allocate_document_id(),
                            egui::pos2(20.0, 30.0),
                            1.0,
                            0.0,
                            0,
                            false,
                        ),
                        asset.clone(),
                    )
                })
                .collect();
            snapshot.groups = vec![Group {
                id: library.allocate_document_id(),
                name: "References".into(),
                members: snapshot.placements.iter().map(Placement::id).collect(),
                container: RectGeometry::new(
                    egui::pos2(20.0, 30.0),
                    egui::vec2(100.0, 100.0),
                    1.0,
                    0.0,
                    0,
                ),
                flow: GroupFlow::Row,
                gap: 8.0,
                layout: crate::data::GroupLayoutSettings {
                    row_gap: 8.0,
                    ..Default::default()
                },
                padding: 12.0,
            }];
            snapshot.stack_order = snapshot
                .placements
                .iter()
                .map(|p| crate::data::CanvasStackItem::Placement(p.id()))
                .collect();
            library.fixture_board_snapshot(&snapshot, board).unwrap();
        }
        let mut accepted = crate::active_board::catalog::AcceptedBoards::default();
        for board in [1, second.id] {
            accepted.ensure_loaded(&library, board).unwrap();
        }
        library
            .archive_assets(&[assets[0].id, assets[1].id])
            .unwrap();
        accepted.remove_assets(&[assets[0].id, assets[1].id]);
        for board in [1, second.id] {
            let document = accepted.document(&library, board).unwrap();
            assert_eq!(
                document.groups[0].members,
                vec![document.placements[0].id()]
            );
        }
        drop(library);
        let reopened = Library::initialize(root.to_path_buf()).unwrap();
        for board in [1, second.id] {
            let document = reopened.load_board_document(board).unwrap();
            assert_eq!(document.placements.len(), 1);
            assert_eq!(document.placements[0].asset_id(), Some(assets[2].id));
            assert_eq!(
                document.groups[0].members,
                vec![document.placements[0].id()],
                "board {board} must not retain archived group members"
            );
            assert_eq!(document.groups[0].flow, GroupFlow::Row);
            assert_eq!(document.groups[0].gap, 8.0);
            assert_eq!(
                document.stack_order,
                vec![crate::data::CanvasStackItem::Placement(
                    document.placements[0].id()
                )]
            );
        }
    }
}

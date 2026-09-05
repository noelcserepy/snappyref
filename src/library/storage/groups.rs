use super::*;
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
            spacing: group.spacing,
            padding: group.padding,
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
            spacing: group.spacing,
            padding: group.padding,
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

impl Library {
    pub fn load_groups(&self) -> Result<Vec<Group>, String> {
        let connection = self.connection()?;
        load_groups_for_board_id(&connection, super::boards::active_board_id(&connection)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            spacing: 8.0,
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
        assert_eq!(
            restored_history.accepted_groups(1),
            Some(vec![group.clone()])
        );
        restored_history.undo(1).unwrap();
        assert_eq!(restored_history.accepted_groups(1), Some(vec![]));
        restored_history.redo(1).unwrap();
        assert_eq!(
            restored_history.accepted_groups(1),
            Some(vec![group.clone()])
        );
        reopened.connection().unwrap().execute_batch("CREATE TRIGGER reject_history BEFORE UPDATE ON board_document_history BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(
            reopened
                .persist_board_unit(&operation(vec![]), &DocumentHistory::default())
                .is_err()
        );
        assert_eq!(reopened.load_groups().unwrap(), vec![group.clone()]);
        assert_eq!(
            reopened.load_document_history().unwrap().accepted_groups(1),
            Some(vec![group])
        );
    }
}

#[derive(Default)]
pub(crate) struct DocumentPersistence {
    last_saved_user_session: Option<crate::data::UserSession>,
    queue: Option<PersistenceQueue>,
}

struct PersistenceQueue {
    tx: std::sync::mpsc::Sender<PersistenceCommand>,
    error_rx: std::sync::mpsc::Receiver<String>,
    worker: Option<std::thread::JoinHandle<()>>,
}

enum PersistenceCommand {
    SaveUserSession(crate::data::UserSession),
    #[cfg(test)]
    CommitCanvasDocumentChanges(crate::canvas::CanvasDocumentChanges),
    CommitBoardUnit {
        operation: crate::library::storage::BoardDurabilityOperation,
        history: crate::active_board::history::DocumentHistory,
    },
    Flush(std::sync::mpsc::Sender<()>),
    Shutdown,
}

#[derive(Default)]
struct PendingPersistenceBatch {
    operations: std::collections::VecDeque<PendingPersistenceOperation>,
    shutdown: bool,
}

enum PendingPersistenceOperation {
    UserSession(crate::data::UserSession),
    #[cfg(test)]
    Document(crate::canvas::CanvasDocumentChanges),
    BoardUnit {
        operation: crate::library::storage::BoardDurabilityOperation,
        history: crate::active_board::history::DocumentHistory,
    },
    Flush(std::sync::mpsc::Sender<()>),
}

#[derive(Clone)]
pub(crate) enum BoardLifecyclePersistence {
    Create {
        document: crate::data::BoardDocumentSnapshot,
        active_board_id: i64,
    },
    Rename {
        board_id: i64,
        name: String,
    },
    Archive {
        board_id: i64,
        active_board_id: i64,
    },
    Restore {
        document: crate::data::BoardDocumentSnapshot,
        active_board_id: i64,
    },
}

impl DocumentPersistence {
    pub(crate) fn queued(library: crate::library::storage::Library) -> Self {
        Self {
            last_saved_user_session: None,
            queue: Some(PersistenceQueue::spawn(library)),
        }
    }

    pub(crate) fn record_saved_user_session(&mut self, session: crate::data::UserSession) {
        self.last_saved_user_session = Some(session);
    }

    #[cfg(test)]
    pub(crate) fn commit_canvas_changes(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        canvas: &mut crate::canvas::CanvasState,
        session: crate::data::UserSession,
    ) -> Result<(), String> {
        let document_result = self.commit_canvas_document_changes(library, canvas);
        let session_result = self.persist_user_session_if_changed(library, session);
        match (document_result, session_result) {
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        }
    }

    pub(crate) fn commit_canvas_changes_unit(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        canvas: &mut crate::canvas::CanvasState,
        board_id: i64,
        history: &crate::active_board::history::DocumentHistory,
        session: crate::data::UserSession,
    ) -> Result<(), String> {
        let document_result = self.commit_canvas_document_unit(library, canvas, board_id, history);
        let session_result = self.persist_user_session_if_changed(library, session);
        match (document_result, session_result) {
            (Err(error), _) | (_, Err(error)) => Err(error),
            _ => Ok(()),
        }
    }

    pub(crate) fn commit_canvas_document_unit(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        canvas: &mut crate::canvas::CanvasState,
        board_id: i64,
        history: &crate::active_board::history::DocumentHistory,
    ) -> Result<bool, String> {
        let Some(library) = library else {
            return Ok(false);
        };
        let changes = canvas.take_document_changes();
        if !changes.has_changes() {
            return Ok(false);
        }
        if self.queue.is_some() {
            self.enqueue_document_unit(board_id, changes, history)?;
        } else {
            library.persist_board_unit(
                &crate::library::storage::BoardDurabilityOperation::Document { board_id, changes },
                history,
            )?;
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn commit_canvas_document_changes(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        canvas: &mut crate::canvas::CanvasState,
    ) -> Result<bool, String> {
        let Some(library) = library else {
            return Ok(false);
        };

        let changes = canvas.take_document_changes();
        let had_changes = changes.has_changes();
        if !had_changes {
            return Ok(false);
        }
        if self.queue.is_some() {
            self.enqueue_document_changes(changes)?;
            return Ok(true);
        }
        write_canvas_document_changes(library, changes).map(|()| true)
    }

    #[cfg(test)]
    pub(crate) fn enqueue_document_changes(
        &mut self,
        changes: crate::canvas::CanvasDocumentChanges,
    ) -> Result<(), String> {
        if !changes.has_changes() {
            return Ok(());
        }
        let Some(queue) = &self.queue else {
            return Err("persistence queue unavailable".to_owned());
        };
        queue.enqueue(PersistenceCommand::CommitCanvasDocumentChanges(changes))
    }

    pub(crate) fn enqueue_document_unit(
        &mut self,
        board_id: i64,
        changes: crate::canvas::CanvasDocumentChanges,
        history: &crate::active_board::history::DocumentHistory,
    ) -> Result<(), String> {
        if !changes.has_changes() {
            return Ok(());
        }
        let Some(queue) = &self.queue else {
            return Err("persistence queue unavailable".to_owned());
        };
        queue.enqueue(PersistenceCommand::CommitBoardUnit {
            operation: crate::library::storage::BoardDurabilityOperation::Document {
                board_id,
                changes,
            },
            history: history.clone(),
        })
    }

    pub(crate) fn apply_board_lifecycle_unit(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        change: BoardLifecyclePersistence,
        history: &crate::active_board::history::DocumentHistory,
    ) -> Result<(), String> {
        let Some(library) = library else {
            return Err("Storage unavailable".to_owned());
        };
        if let Some(queue) = &self.queue {
            return queue.enqueue(PersistenceCommand::CommitBoardUnit {
                operation: crate::library::storage::BoardDurabilityOperation::Lifecycle(change),
                history: history.clone(),
            });
        }
        library.persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::Lifecycle(change),
            history,
        )
    }

    #[cfg(test)]
    pub(crate) fn persist_history(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        history: &crate::active_board::history::DocumentHistory,
    ) -> Result<(), String> {
        let Some(library) = library else {
            return Err("Storage unavailable".to_owned());
        };
        if let Some(queue) = &self.queue {
            return queue.enqueue(PersistenceCommand::CommitBoardUnit {
                operation: crate::library::storage::BoardDurabilityOperation::HistoryOnly,
                history: history.clone(),
            });
        }
        library.persist_board_unit(
            &crate::library::storage::BoardDurabilityOperation::HistoryOnly,
            history,
        )
    }

    pub(crate) fn save_user_session(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        session: crate::data::UserSession,
    ) -> Result<(), String> {
        let Some(library) = library else {
            return Ok(());
        };
        if let Some(queue) = &self.queue {
            queue.enqueue(PersistenceCommand::SaveUserSession(session.clone()))?;
            self.last_saved_user_session = Some(session);
            return Ok(());
        }
        library.save_user_session(&session)?;
        self.last_saved_user_session = Some(session);
        Ok(())
    }

    pub(crate) fn persist_user_session_if_changed(
        &mut self,
        library: Option<&crate::library::storage::Library>,
        session: crate::data::UserSession,
    ) -> Result<(), String> {
        if self.last_saved_user_session.as_ref() == Some(&session) {
            return Ok(());
        }
        self.save_user_session(library, session)
    }

    pub(crate) fn user_session_changed(&self, session: &crate::data::UserSession) -> bool {
        self.last_saved_user_session.as_ref() != Some(session)
    }

    pub(crate) fn flush(&mut self) -> Result<(), String> {
        let Some(queue) = &self.queue else {
            return Ok(());
        };
        queue.flush()
    }
}

impl PersistenceQueue {
    fn spawn(library: crate::library::storage::Library) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (error_tx, error_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut batch = PendingPersistenceBatch::default();
            loop {
                if batch.operations.is_empty() && !batch.shutdown {
                    let Ok(command) = rx.recv() else {
                        batch.shutdown = true;
                        if batch.operations.is_empty() {
                            break;
                        }
                        continue;
                    };
                    batch.push(command);
                }
                while !batch.shutdown {
                    match rx.try_recv() {
                        Ok(command) => batch.push(command),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            batch.shutdown = true;
                            break;
                        }
                    }
                }
                let persisted = batch.persist(&library, &error_tx);
                if batch.shutdown && batch.operations.is_empty() {
                    break;
                }
                if !persisted {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        });
        Self {
            tx,
            error_rx,
            worker: Some(worker),
        }
    }

    fn enqueue(&self, command: PersistenceCommand) -> Result<(), String> {
        self.tx
            .send(command)
            .map_err(|error| format!("failed to enqueue persistence work: {error}"))
    }

    fn flush(&self) -> Result<(), String> {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        self.tx
            .send(PersistenceCommand::Flush(done_tx))
            .map_err(|error| format!("failed to enqueue persistence flush: {error}"))?;
        done_rx
            .recv()
            .map_err(|error| format!("failed to wait for persistence flush: {error}"))?;
        self.drain_errors()
    }

    fn drain_errors(&self) -> Result<(), String> {
        let mut last_error = None;
        while let Ok(error) = self.error_rx.try_recv() {
            last_error = Some(error);
        }
        match last_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for PersistenceQueue {
    fn drop(&mut self) {
        let _ = self.tx.send(PersistenceCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl PendingPersistenceBatch {
    fn push(&mut self, command: PersistenceCommand) {
        match command {
            PersistenceCommand::SaveUserSession(session) => match self.operations.back_mut() {
                Some(PendingPersistenceOperation::UserSession(pending)) => *pending = session,
                _ => self
                    .operations
                    .push_back(PendingPersistenceOperation::UserSession(session)),
            },
            #[cfg(test)]
            PersistenceCommand::CommitCanvasDocumentChanges(changes) => {
                match self.operations.back_mut() {
                    Some(PendingPersistenceOperation::Document(pending)) => {
                        merge_document_changes(pending, changes);
                    }
                    _ => self
                        .operations
                        .push_back(PendingPersistenceOperation::Document(changes)),
                }
            }
            PersistenceCommand::CommitBoardUnit { operation, history } => {
                self.operations
                    .push_back(PendingPersistenceOperation::BoardUnit { operation, history });
            }
            PersistenceCommand::Flush(done_tx) => {
                self.operations
                    .push_back(PendingPersistenceOperation::Flush(done_tx));
            }
            PersistenceCommand::Shutdown => {
                self.shutdown = true;
            }
        }
    }

    fn persist(
        &mut self,
        library: &crate::library::storage::Library,
        error_tx: &std::sync::mpsc::Sender<String>,
    ) -> bool {
        while let Some(operation) = self.operations.front() {
            let result = match operation {
                PendingPersistenceOperation::UserSession(session) => {
                    library.save_user_session(session)
                }
                #[cfg(test)]
                PendingPersistenceOperation::Document(changes) => {
                    write_canvas_document_changes(library, changes.clone())
                }
                PendingPersistenceOperation::BoardUnit { operation, history } => {
                    library.persist_board_unit(&operation, &history)
                }
                PendingPersistenceOperation::Flush(done_tx) => {
                    let _ = done_tx.send(());
                    Ok(())
                }
            };
            if let Err(error) = result {
                let _ = error_tx.send(error);
                return false;
            }
            self.operations.pop_front();
        }
        true
    }
}

#[cfg(test)]
fn merge_document_changes(
    pending: &mut crate::canvas::CanvasDocumentChanges,
    changes: crate::canvas::CanvasDocumentChanges,
) {
    for placement_id in changes.archived_placement_ids {
        if !pending.archived_placement_ids.contains(&placement_id) {
            pending.archived_placement_ids.push(placement_id);
        }
    }
    for item in changes.updated_placements {
        if let Some(existing) = pending
            .updated_placements
            .iter_mut()
            .find(|existing| existing.id == item.id)
        {
            *existing = item;
        } else {
            pending.updated_placements.push(item);
        }
    }
    for annotation in changes.updated_annotations {
        if let Some(existing) = pending
            .updated_annotations
            .iter_mut()
            .find(|existing| existing.id == annotation.id)
        {
            *existing = annotation;
        } else {
            pending.updated_annotations.push(annotation);
        }
    }
    for annotation_id in changes.archived_annotation_ids {
        if !pending.archived_annotation_ids.contains(&annotation_id) {
            pending.archived_annotation_ids.push(annotation_id);
        }
    }
    if changes.stack_order.is_some() {
        pending.stack_order = changes.stack_order;
    }
}

#[cfg(test)]
fn write_canvas_document_changes(
    library: &crate::library::storage::Library,
    changes: crate::canvas::CanvasDocumentChanges,
) -> Result<(), String> {
    let mut last_error = None;
    if !changes.archived_annotation_ids.is_empty()
        && let Err(error) = library.archive_canvas_annotations(&changes.archived_annotation_ids)
    {
        last_error = Some(error);
    }

    for item in changes.updated_placements {
        if let Err(error) = library.update_placement_document_state(&item) {
            last_error = Some(error);
        }
    }

    for annotation in changes.updated_annotations {
        if let Err(error) = library.update_canvas_annotation(&annotation) {
            last_error = Some(error);
        }
    }

    if let Some(stack_order) = changes.stack_order
        && let Err(error) = library.save_canvas_stack_order(&stack_order)
    {
        last_error = Some(error);
    }

    match last_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentPersistence;

    #[test]
    fn unchanged_session_skips_save() {
        let mut persistence = DocumentPersistence::default();
        let session = crate::data::UserSession::default();
        persistence.record_saved_user_session(session.clone());

        let result = persistence.persist_user_session_if_changed(None, session);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn missing_library_does_not_drain_canvas_changes() {
        let mut persistence = DocumentPersistence::default();
        let mut canvas = crate::canvas::CanvasState::default();
        canvas.set_canvas_stack_order(vec![crate::data::CanvasStackItem::Annotation(7)]);

        let result = persistence.commit_canvas_document_changes(None, &mut canvas);

        assert_eq!(result, Ok(false));
        assert!(canvas.take_document_changes().stack_order.is_some());
    }

    #[test]
    fn queued_session_persistence_is_durable_after_flush() {
        let root =
            std::env::temp_dir().join(format!("snappyref-queued-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = crate::library::storage::Library::initialize(root.clone()).unwrap();
        let mut persistence = DocumentPersistence::queued(library.clone());
        let session = crate::data::UserSession {
            viewport: crate::data::ViewportState {
                camera_pan_x: 12.0,
                camera_pan_y: 34.0,
                camera_zoom: 2.0,
                canvas_background: crate::data::CanvasBackground::Dots,
            },
            ..Default::default()
        };

        persistence
            .save_user_session(Some(&library), session)
            .unwrap();
        persistence.flush().unwrap();

        let stored = library.load_user_session().unwrap();
        assert_eq!(stored.viewport.camera_pan_x, 12.0);
        assert_eq!(stored.viewport.camera_pan_y, 34.0);
        assert_eq!(stored.viewport.camera_zoom, 2.0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn queued_document_persistence_is_durable_after_flush() {
        let root =
            std::env::temp_dir().join(format!("snappyref-queued-document-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = crate::library::storage::Library::initialize(root.clone()).unwrap();
        let source = root.join("source.png");
        std::fs::write(&source, [255, 255, 255, 255]).unwrap();
        let mut record = library
            .persist_imported_image(
                &source,
                "image/png",
                1,
                1,
                4,
                eframe::egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let replacement_source = root.join("replacement.png");
        std::fs::write(&replacement_source, [0, 0, 0, 255]).unwrap();
        let replacement = library
            .persist_imported_image(
                &replacement_source,
                "image/png",
                1,
                1,
                4,
                eframe::egui::pos2(100.0, 100.0),
                1.0,
            )
            .unwrap();
        record.state.asset_id = replacement.state.asset_id;
        record.state.geometry.center.x = 42.0;
        record.state.geometry.center.y = 64.0;
        let changes = crate::canvas::CanvasDocumentChanges {
            updated_placements: vec![record.state.clone()],
            ..Default::default()
        };
        let mut persistence = DocumentPersistence::queued(library.clone());

        persistence.enqueue_document_changes(changes).unwrap();
        persistence.flush().unwrap();

        let stored = library
            .load_placements()
            .unwrap()
            .into_iter()
            .find(|placement| placement.state.id == record.state.id)
            .unwrap();
        assert_eq!(stored.state.asset_id, replacement.state.asset_id);
        assert_eq!(stored.state.geometry.center.x, 42.0);
        assert_eq!(stored.state.geometry.center.y, 64.0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn queued_document_persistence_coalesces_item_updates_before_flush() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-queued-document-coalesces-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let library = crate::library::storage::Library::initialize(root.clone()).unwrap();
        let source = root.join("source.png");
        std::fs::write(&source, [255, 255, 255, 255]).unwrap();
        let mut record = library
            .persist_imported_image(
                &source,
                "image/png",
                1,
                1,
                4,
                eframe::egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let mut persistence = DocumentPersistence::queued(library.clone());

        record.state.geometry.center.x = 10.0;
        persistence
            .enqueue_document_changes(crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![record.state.clone()],
                ..Default::default()
            })
            .unwrap();
        record.state.geometry.center.x = 30.0;
        record.state.geometry.center.y = 40.0;
        persistence
            .enqueue_document_changes(crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![record.state.clone()],
                ..Default::default()
            })
            .unwrap();
        persistence.flush().unwrap();

        let stored = library.load_placements().unwrap();
        assert_eq!(stored[0].state.geometry.center.x, 30.0);
        assert_eq!(stored[0].state.geometry.center.y, 40.0);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn queued_failure_does_not_reject_later_changes_or_skip_flush() {
        let root = std::env::temp_dir().join(format!(
            "snappyref-queued-failure-recovery-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let library = crate::library::storage::Library::initialize(root.clone()).unwrap();
        let source = root.join("source.png");
        std::fs::write(&source, [255, 255, 255, 255]).unwrap();
        let record = library
            .persist_imported_image(
                &source,
                "image/png",
                1,
                1,
                4,
                eframe::egui::pos2(0.0, 0.0),
                1.0,
            )
            .unwrap();
        let mut persistence = DocumentPersistence::queued(library.clone());

        let mut invalid = record.state.clone();
        invalid.asset_id = Some(i64::MAX);
        persistence
            .enqueue_document_changes(crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![invalid],
                ..Default::default()
            })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut valid = record.state.clone();
        valid.geometry.center.x = 42.0;
        persistence
            .enqueue_document_changes(crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![valid],
                ..Default::default()
            })
            .unwrap();

        assert!(persistence.flush().is_err());
        assert_eq!(
            library.load_placements().unwrap()[0]
                .state
                .geometry
                .center
                .x,
            42.0
        );

        let _ = std::fs::remove_dir_all(root);
    }
}

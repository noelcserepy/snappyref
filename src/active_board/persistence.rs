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
    CommitCanvasDocumentChanges(crate::canvas::CanvasDocumentChanges),
    Flush(std::sync::mpsc::Sender<()>),
    Shutdown,
}

#[derive(Default)]
struct PendingPersistenceBatch {
    user_session: Option<crate::data::UserSession>,
    document_changes: crate::canvas::CanvasDocumentChanges,
    flushes: Vec<std::sync::mpsc::Sender<()>>,
    shutdown: bool,
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
        if let Some(queue) = &self.queue {
            queue.enqueue(PersistenceCommand::CommitCanvasDocumentChanges(changes))?;
            return Ok(true);
        }
        write_canvas_document_changes(library, changes).map(|()| true)
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

    #[cfg(test)]
    fn enqueue_canvas_document_changes_for_test(
        &mut self,
        changes: crate::canvas::CanvasDocumentChanges,
    ) -> Result<(), String> {
        let Some(queue) = &self.queue else {
            return Err("persistence queue unavailable".to_owned());
        };
        queue.enqueue(PersistenceCommand::CommitCanvasDocumentChanges(changes))
    }
}

impl PersistenceQueue {
    fn spawn(library: crate::library::storage::Library) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (error_tx, error_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            while let Ok(command) = rx.recv() {
                let mut batch = PendingPersistenceBatch::default();
                batch.push(command);
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
                if let Err(error) = batch.persist(&library) {
                    let _ = error_tx.send(error);
                }
                batch.finish_flushes();
                if batch.shutdown {
                    break;
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
        self.drain_errors()?;
        self.tx
            .send(command)
            .map_err(|error| format!("failed to enqueue persistence work: {error}"))
    }

    fn flush(&self) -> Result<(), String> {
        self.drain_errors()?;
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
            PersistenceCommand::SaveUserSession(session) => {
                self.user_session = Some(session);
            }
            PersistenceCommand::CommitCanvasDocumentChanges(changes) => {
                self.merge_document_changes(changes);
            }
            PersistenceCommand::Flush(done_tx) => {
                self.flushes.push(done_tx);
            }
            PersistenceCommand::Shutdown => {
                self.shutdown = true;
            }
        }
    }

    fn merge_document_changes(&mut self, changes: crate::canvas::CanvasDocumentChanges) {
        for item in changes.updated_placements {
            if let Some(existing) = self
                .document_changes
                .updated_placements
                .iter_mut()
                .find(|existing| existing.id == item.id)
            {
                *existing = item;
            } else {
                self.document_changes.updated_placements.push(item);
            }
        }
        for annotation in changes.updated_annotations {
            if let Some(existing) = self
                .document_changes
                .updated_annotations
                .iter_mut()
                .find(|existing| existing.id == annotation.id)
            {
                *existing = annotation;
            } else {
                self.document_changes.updated_annotations.push(annotation);
            }
        }
        for annotation_id in changes.archived_annotation_ids {
            if !self
                .document_changes
                .archived_annotation_ids
                .contains(&annotation_id)
            {
                self.document_changes
                    .archived_annotation_ids
                    .push(annotation_id);
            }
        }
        if changes.stack_order.is_some() {
            self.document_changes.stack_order = changes.stack_order;
        }
    }

    fn persist(&mut self, library: &crate::library::storage::Library) -> Result<(), String> {
        let mut last_error = None;
        let document_changes = std::mem::take(&mut self.document_changes);
        if document_changes.has_changes()
            && let Err(error) = write_canvas_document_changes(library, document_changes)
        {
            last_error = Some(error);
        }
        if let Some(session) = self.user_session.take()
            && let Err(error) = library.save_user_session(&session)
        {
            last_error = Some(error);
        }
        match last_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn finish_flushes(&mut self) {
        for done_tx in self.flushes.drain(..) {
            let _ = done_tx.send(());
        }
    }
}

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
        if let Err(error) = library.update_placement_state(&item) {
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
        record.state.geometry.center.x = 42.0;
        record.state.geometry.center.y = 64.0;
        let changes = crate::canvas::CanvasDocumentChanges {
            updated_placements: vec![record.state.clone()],
            ..Default::default()
        };
        let mut persistence = DocumentPersistence::queued(library.clone());

        persistence
            .enqueue_canvas_document_changes_for_test(changes)
            .unwrap();
        persistence.flush().unwrap();

        let stored = library.load_placements().unwrap();
        assert_eq!(stored[0].state.geometry.center.x, 42.0);
        assert_eq!(stored[0].state.geometry.center.y, 64.0);

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
            .enqueue_canvas_document_changes_for_test(crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![record.state.clone()],
                ..Default::default()
            })
            .unwrap();
        record.state.geometry.center.x = 30.0;
        record.state.geometry.center.y = 40.0;
        persistence
            .enqueue_canvas_document_changes_for_test(crate::canvas::CanvasDocumentChanges {
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
}

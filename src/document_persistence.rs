#[derive(Default)]
pub(crate) struct DocumentPersistence {
    last_saved_user_session: Option<crate::data::UserSession>,
}

impl DocumentPersistence {
    pub(crate) fn record_saved_user_session(&mut self, session: crate::data::UserSession) {
        self.last_saved_user_session = Some(session);
    }

    pub(crate) fn commit_canvas_changes(
        &mut self,
        library: Option<&crate::storage::Library>,
        canvas: &mut crate::canvas::CanvasState,
        session: crate::data::UserSession,
    ) -> Result<(), String> {
        let Some(library) = library else {
            return Ok(());
        };

        let mut last_error = None;
        let changes = canvas.take_document_changes();
        if !changes.archived_annotation_ids.is_empty()
            && let Err(error) = library.archive_canvas_annotations(&changes.archived_annotation_ids)
        {
            last_error = Some(error);
        }

        for item in changes.updated_items {
            if let Err(error) = library.update_board_item(&item) {
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

        if let Err(error) = self.persist_user_session_if_changed(Some(library), session) {
            last_error = Some(error);
        }

        match last_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) fn save_user_session(
        &mut self,
        library: Option<&crate::storage::Library>,
        session: crate::data::UserSession,
    ) -> Result<(), String> {
        let Some(library) = library else {
            return Ok(());
        };
        library.save_user_session(&session)?;
        self.last_saved_user_session = Some(session);
        Ok(())
    }

    pub(crate) fn persist_user_session_if_changed(
        &mut self,
        library: Option<&crate::storage::Library>,
        session: crate::data::UserSession,
    ) -> Result<(), String> {
        if self.last_saved_user_session.as_ref() == Some(&session) {
            return Ok(());
        }
        self.save_user_session(library, session)
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

        let result = persistence.commit_canvas_changes(
            None,
            &mut canvas,
            crate::data::UserSession::default(),
        );

        assert_eq!(result, Ok(()));
        assert!(canvas.take_document_changes().stack_order.is_some());
    }
}

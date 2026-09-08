//! Helpers shared by unit tests across modules.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Unique scratch directory under the OS temp dir, removed on drop so a
/// panicking test still cleans up. Derefs to `Path`.
pub(crate) struct TempRoot(PathBuf);

impl TempRoot {
    pub(crate) fn new(prefix: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            TEMP_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl std::ops::Deref for TempRoot {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl crate::library::storage::Library {
    pub(crate) fn fixture_placement(
        &self,
        board_id: i64,
        placement: crate::data::Placement,
    ) -> Result<crate::data::Placement, String> {
        let mut stack = self.load_board_document(board_id)?.stack_order;
        stack.push(crate::data::CanvasStackItem::Placement(placement.id()));
        self.fixture_board_document(
            board_id,
            crate::canvas::CanvasDocumentChanges {
                updated_placements: vec![placement.clone()],
                stack_order: Some(stack),
                ..Default::default()
            },
        )?;
        Ok(placement)
    }
}

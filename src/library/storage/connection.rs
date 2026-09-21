use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::TransactionBehavior;
use rusqlite::params;
use std::fs;
use std::path::PathBuf;

use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};
use std::time::Duration;

const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(2);

/// Managed Library filesystem layout.
///
/// The database and app-owned derivatives live together under the Library
/// root. User-owned originals are copied into `originals/`; canvas thumbnails
/// and raster caches live under `thumbs/`; the SQLite catalog is
/// `library.db`.
#[derive(Clone, Debug)]
pub struct Library {
    pub(super) originals_dir: PathBuf,
    pub(super) thumbs_dir: PathBuf,
    pub(super) db_path: PathBuf,
    document_ids: Arc<AtomicI64>,
}

impl Library {
    pub fn initialize(root: PathBuf) -> Result<Self, String> {
        let originals_dir = root.join("originals");
        let thumbs_dir = root.join("thumbs");
        let db_path = root.join("library.db");

        fs::create_dir_all(&originals_dir)
            .map_err(|error| format!("failed to create originals dir: {error}"))?;
        fs::create_dir_all(&thumbs_dir)
            .map_err(|error| format!("failed to create thumbs dir: {error}"))?;

        let library = Self {
            originals_dir,
            thumbs_dir,
            db_path,
            document_ids: Arc::new(AtomicI64::new(1)),
        };
        library.initialize_schema()?;
        library.rebase_managed_originals()?;
        let connection = library.connection()?;
        let next_id = connection
            .query_row(
                "SELECT MAX(id) + 1 FROM (
                    SELECT id FROM boards
                    UNION ALL SELECT id FROM board_items
                    UNION ALL SELECT id FROM canvas_annotations
                    UNION ALL SELECT high_watermark AS id FROM document_identity
                 )",
                [],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| format!("failed to initialize Board identity allocator: {error}"))?
            .unwrap_or(1);
        library.document_ids.store(next_id, Ordering::Relaxed);
        Ok(library)
    }

    fn rebase_managed_originals(&self) -> Result<(), String> {
        let mut connection = self.connection()?;
        let separator = std::path::MAIN_SEPARATOR;
        let pattern = format!("%{separator}originals{separator}%");
        // Probe for any managed original stored under another root before
        // scanning, so an unmoved Library pays one indexed-free lookup at open.
        let current_prefix = format!("{}{separator}", self.originals_dir.to_string_lossy());
        let stale: Option<i64> = connection
            .query_row(
                "SELECT id FROM assets
                 WHERE original_path LIKE ?1
                   AND SUBSTR(original_path, 1, ?2) <> ?3
                 LIMIT 1",
                params![
                    &pattern,
                    current_prefix.chars().count() as i64,
                    &current_prefix
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("failed to locate managed originals: {error}"))?;
        if stale.is_none() {
            return Ok(());
        }
        let paths = connection
            .prepare("SELECT id, original_path FROM assets WHERE original_path LIKE ?1")
            .map_err(|error| error.to_string())?
            .query_map([pattern], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let relocations = paths
            .into_iter()
            .filter_map(|(id, stored)| {
                let stored = PathBuf::from(stored);
                let parent = stored.parent()?;
                if parent == self.originals_dir || parent.file_name()? != "originals" {
                    return None;
                }
                let relocated = self.originals_dir.join(stored.file_name()?);
                relocated.is_file().then_some((id, relocated))
            })
            .collect::<Vec<_>>();
        if relocations.is_empty() {
            return Ok(());
        }
        let transaction = write_transaction(&mut connection, "managed original relocation")?;
        for (id, relocated) in relocations {
            transaction
                .execute(
                    "UPDATE assets SET original_path = ?1 WHERE id = ?2",
                    params![relocated.to_string_lossy(), id],
                )
                .map_err(|error| format!("failed to relocate managed original: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit original relocation: {error}"))?;
        Ok(())
    }

    pub(crate) fn allocate_document_id(&self) -> i64 {
        self.document_ids.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn persist_document_identity(
        &self,
        transaction: &Transaction<'_>,
    ) -> Result<(), String> {
        transaction.execute(
            "INSERT INTO document_identity (id, high_watermark) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET high_watermark = MAX(high_watermark, excluded.high_watermark)",
            params![self.document_ids.load(Ordering::Relaxed) - 1],
        ).map_err(|error| format!("failed to save document identity watermark: {error}"))?;
        Ok(())
    }

    pub(super) fn connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.db_path)
            .map_err(|error| format!("failed to open library database: {error}"))?;
        connection
            .busy_timeout(SQLITE_BUSY_TIMEOUT)
            .map_err(|error| format!("failed to configure library database timeout: {error}"))?;
        Ok(connection)
    }
}

/// Every Library write starts as an Immediate transaction so the writer takes
/// the lock up front instead of failing with SQLITE_BUSY at commit.
pub(super) fn write_transaction<'c>(
    connection: &'c mut Connection,
    label: &str,
) -> Result<Transaction<'c>, String> {
    connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("failed to start {label} transaction: {error}"))
}

#[cfg(test)]
mod tests {
    use super::Library;

    #[test]
    fn document_identity_does_not_reuse_removed_group_ids_after_restart() {
        let root = crate::test_support::TempRoot::new("snappyref-group-identity");
        let library = Library::initialize(root.to_path_buf()).unwrap();
        let group_id = library.allocate_document_id();
        // An ungrouped document and cleared history still retain allocated IDs.
        library
            .persist_board_unit(
                &crate::library::storage::BoardDurabilityOperation::Document {
                    board_id: 1,
                    changes: crate::canvas::CanvasDocumentChanges {
                        groups: Some(vec![]),
                        ..Default::default()
                    },
                },
                &crate::active_board::history::DocumentHistory::default(),
            )
            .unwrap();
        let reopened = Library::initialize(root.to_path_buf()).unwrap();
        assert!(reopened.allocate_document_id() > group_id);
    }
}

use super::*;
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
        let connection = library.connection()?;
        let next_id = connection
            .query_row(
                "SELECT MAX(id) + 1 FROM (
                    SELECT id FROM boards
                    UNION ALL SELECT id FROM board_items
                    UNION ALL SELECT id FROM canvas_annotations
                 )",
                [],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| format!("failed to initialize Board identity allocator: {error}"))?
            .unwrap_or(1);
        library.document_ids.store(next_id, Ordering::Relaxed);
        Ok(library)
    }

    pub(crate) fn allocate_document_id(&self) -> i64 {
        self.document_ids.fetch_add(1, Ordering::Relaxed)
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

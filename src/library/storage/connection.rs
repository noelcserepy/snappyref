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
    identities: Arc<DocumentIdentityAllocator>,
}

#[derive(Debug)]
struct DocumentIdentityAllocator {
    next_board_id: AtomicI64,
    next_placement_id: AtomicI64,
    next_annotation_id: AtomicI64,
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

        let mut library = Self {
            originals_dir,
            thumbs_dir,
            db_path,
            identities: Arc::new(DocumentIdentityAllocator {
                next_board_id: AtomicI64::new(1),
                next_placement_id: AtomicI64::new(1),
                next_annotation_id: AtomicI64::new(1),
            }),
        };
        library.initialize_schema()?;
        let connection = library.connection()?;
        let next_id = |table: &str| -> Result<i64, String> {
            connection
                .query_row(
                    &format!("SELECT COALESCE(MAX(id), 0) + 1 FROM {table}"),
                    [],
                    |row| row.get(0),
                )
                .map_err(|error| {
                    format!("failed to initialize {table} identity allocation: {error}")
                })
        };
        library.identities = Arc::new(DocumentIdentityAllocator {
            next_board_id: AtomicI64::new(next_id("boards")?),
            next_placement_id: AtomicI64::new(next_id("board_items")?),
            next_annotation_id: AtomicI64::new(next_id("canvas_annotations")?),
        });
        Ok(library)
    }

    pub(crate) fn allocate_board_id(&self) -> i64 {
        self.identities
            .next_board_id
            .fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn allocate_placement_id(&self) -> i64 {
        self.identities
            .next_placement_id
            .fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn allocate_annotation_id(&self) -> i64 {
        self.identities
            .next_annotation_id
            .fetch_add(1, Ordering::Relaxed)
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

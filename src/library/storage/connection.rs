use super::*;
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
        };
        library.initialize_schema()?;
        Ok(library)
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

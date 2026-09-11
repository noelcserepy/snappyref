use super::boards::{active_board_id, board_is_non_archived};
use super::connection::Library;
use crate::data::CanvasBackground;
use crate::data::UserSession;
use crate::data::ViewportState;
use crate::data::WorkspaceSettings;
use rusqlite::Connection;
use rusqlite::params;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, Default)]
pub(super) struct StoredUserSessionPayload {
    #[serde(default)]
    window: crate::data::WindowState,
    #[serde(default)]
    workspace_settings: WorkspaceSettings,
    #[serde(default)]
    sidebar: StoredSidebarState,
    #[serde(default)]
    recent_searches: Vec<String>,
}

impl StoredUserSessionPayload {
    pub(super) fn from_session(session: &UserSession) -> Self {
        Self {
            window: session.window.clone(),
            workspace_settings: session.settings.clone(),
            sidebar: StoredSidebarState {
                open: session.sidebar_open,
                tab: session.sidebar_tab.clone(),
            },
            recent_searches: session.recent_searches.clone(),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct StoredSidebarState {
    #[serde(default)]
    open: bool,
    #[serde(default = "default_sidebar_tab")]
    tab: String,
}

impl Default for StoredSidebarState {
    fn default() -> Self {
        Self {
            open: false,
            tab: default_sidebar_tab(),
        }
    }
}

fn default_sidebar_tab() -> String {
    "settings".to_owned()
}

pub(super) fn board_viewport_state(
    connection: &Connection,
    board_id: i64,
) -> Result<ViewportState, rusqlite::Error> {
    connection.query_row(
        "SELECT camera_pan_x, camera_pan_y, camera_zoom, canvas_background
         FROM boards
         WHERE id = ?1",
        params![board_id],
        |row| {
            let background = row
                .get::<_, String>(3)
                .ok()
                .and_then(|value| CanvasBackground::from_str(&value))
                .unwrap_or_default();
            Ok(ViewportState {
                camera_pan_x: row.get(0)?,
                camera_pan_y: row.get(1)?,
                camera_zoom: row.get(2)?,
                canvas_background: background,
            })
        },
    )
}

fn decode_user_session_payload(payload: Option<String>) -> StoredUserSessionPayload {
    payload
        .filter(|payload| !payload.trim().is_empty())
        .and_then(|payload| serde_json::from_str(&payload).ok())
        .unwrap_or_default()
}

pub(super) fn encode_user_session_payload(session: &UserSession) -> Result<String, String> {
    serde_json::to_string(&StoredUserSessionPayload::from_session(session))
        .map_err(|error| format!("failed to encode user session payload: {error}"))
}

impl Library {
    pub fn load_user_session(&self) -> Result<UserSession, String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let viewport = board_viewport_state(&connection, active_board_id)
            .map_err(|error| format!("failed to load active board viewport: {error}"))?;
        let payload = match connection.query_row(
            "SELECT session_payload FROM user_session WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(payload) => payload,
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(format!("failed to load user session: {error}")),
        };
        let stored = decode_user_session_payload(payload);
        Ok(UserSession {
            active_board_id,
            viewport,
            window: stored.window,
            settings: stored.workspace_settings,
            sidebar_open: stored.sidebar.open,
            sidebar_tab: stored.sidebar.tab,
            recent_searches: stored.recent_searches,
        })
    }

    pub fn save_user_session(&self, session: &UserSession) -> Result<(), String> {
        let connection = self.connection()?;
        let active_board_id = if board_is_non_archived(&connection, session.active_board_id)? {
            session.active_board_id
        } else {
            active_board_id(&connection)?
        };
        let session_payload = encode_user_session_payload(session)?;
        connection
            .execute(
                "INSERT INTO user_session (
                    id, session_payload, active_board_id
                 )
                 VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET
                    session_payload = excluded.session_payload,
                    active_board_id = excluded.active_board_id",
                params![session_payload, active_board_id],
            )
            .map_err(|error| format!("failed to save user session: {error}"))?;

        connection
            .execute(
                "UPDATE boards
                 SET camera_pan_x = ?1,
                     camera_pan_y = ?2,
                     camera_zoom = ?3,
                     canvas_background = ?4
                 WHERE id = ?5",
                params![
                    session.viewport.camera_pan_x,
                    session.viewport.camera_pan_y,
                    session.viewport.camera_zoom,
                    session.viewport.canvas_background.as_str(),
                    active_board_id,
                ],
            )
            .map_err(|error| format!("failed to save board viewport: {error}"))?;

        Ok(())
    }
}

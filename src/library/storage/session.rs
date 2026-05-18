use super::boards::{active_board_id, board_is_non_archived};
use super::*;

const USER_SESSION_PAYLOAD_VERSION: u32 = 1;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(super) struct StoredUserSessionPayload {
    #[serde(default = "current_user_session_payload_version")]
    version: u32,
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
            version: USER_SESSION_PAYLOAD_VERSION,
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

impl Default for StoredUserSessionPayload {
    fn default() -> Self {
        Self {
            version: USER_SESSION_PAYLOAD_VERSION,
            window: crate::data::WindowState::default(),
            workspace_settings: WorkspaceSettings::default(),
            sidebar: StoredSidebarState::default(),
            recent_searches: Vec::new(),
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

fn current_user_session_payload_version() -> u32 {
    USER_SESSION_PAYLOAD_VERSION
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

pub(super) fn legacy_user_session_from_row(
    row: &rusqlite::Row<'_>,
    active_board_id: i64,
    viewport: ViewportState,
) -> Result<UserSession, rusqlite::Error> {
    Ok(UserSession {
        active_board_id,
        viewport,
        window: crate::data::WindowState::default(),
        settings: WorkspaceSettings {
            auto_image_scaling: row.get::<_, i64>(0)? != 0,
            placement_gap: row.get(1)?,
            import_target_canvas_area: row
                .get::<_, f32>(2)
                .ok()
                .map(|value| value.max(1.0))
                .unwrap_or(crate::data::DEFAULT_IMPORT_TARGET_CANVAS_AREA),
            canvas_image_max_side: row
                .get::<_, i64>(4)
                .ok()
                .map(|value| value.max(1) as u32)
                .unwrap_or(crate::data::DEFAULT_CANVAS_IMAGE_MAX_SIDE),
            semantic_search_enabled: row.get::<_, i64>(3).unwrap_or(0) != 0,
            semantic_search_first_launch_prompt_seen: false,
            rotation_snap_degrees: row
                .get::<_, f32>(5)
                .ok()
                .map(|value| value.max(0.0))
                .unwrap_or(crate::data::DEFAULT_ROTATION_SNAP_DEGREES),
            default_textbox_background: row.get::<_, i64>(6).unwrap_or(0) != 0,
            pointer_mode: crate::data::PointerMode::default(),
            always_on_top: false,
            last_update_check_at: None,
        },
        sidebar_open: row.get::<_, i64>(7).unwrap_or(0) != 0,
        sidebar_tab: row
            .get::<_, String>(8)
            .unwrap_or_else(|_| default_sidebar_tab()),
        recent_searches: Vec::new(),
    })
}

pub(super) fn apply_user_session_payload(
    mut session: UserSession,
    payload: Option<String>,
) -> UserSession {
    let Some(payload) = payload else {
        return session;
    };
    if payload.trim().is_empty() {
        return session;
    }

    let Ok(stored) = serde_json::from_str::<StoredUserSessionPayload>(&payload) else {
        return session;
    };

    session.settings = stored.workspace_settings;
    session.window = stored.window;
    session.sidebar_open = stored.sidebar.open;
    session.sidebar_tab = stored.sidebar.tab;
    session.recent_searches = stored.recent_searches;
    session
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
        let mut statement = connection
            .prepare(
                "SELECT
                    auto_image_scaling,
                    placement_gap,
                    import_target_canvas_area,
                    semantic_search_enabled,
                    canvas_image_max_side,
                    rotation_snap_degrees,
                    default_textbox_background,
                    sidebar_open,
                    sidebar_tab,
                    session_payload
                 FROM user_session
                 WHERE id = 1",
            )
            .map_err(|error| format!("failed to prepare user-session query: {error}"))?;

        let session = statement.query_row([], |row| {
            let legacy = legacy_user_session_from_row(row, active_board_id, viewport.clone())?;
            let payload = row.get::<_, Option<String>>(9).ok().flatten();
            Ok(apply_user_session_payload(legacy, payload))
        });

        match session {
            Ok(session) => Ok(session),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let mut session = UserSession::default();
                session.active_board_id = active_board_id;
                session.viewport = viewport;
                Ok(session)
            }
            Err(error) => Err(format!("failed to load user session: {error}")),
        }
    }

    pub fn load_session_viewport(&self) -> Result<(i64, ViewportState), String> {
        let connection = self.connection()?;
        let active_board_id = active_board_id(&connection)?;
        let viewport = board_viewport_state(&connection, active_board_id)
            .map_err(|error| format!("failed to load active board viewport: {error}"))?;
        Ok((active_board_id, viewport))
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
                    id,
                    camera_pan_x,
                    camera_pan_y,
                    camera_zoom,
                    auto_image_scaling,
                    placement_gap,
                    import_target_canvas_area,
                    canvas_background,
                    semantic_search_enabled,
                    canvas_image_mode,
                    canvas_image_max_side,
                    rotation_snap_degrees,
                    default_textbox_background,
                    sidebar_open,
                    sidebar_tab,
                    session_payload,
                    active_board_id
                 )
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                 ON CONFLICT(id) DO UPDATE SET
                    camera_pan_x = excluded.camera_pan_x,
                    camera_pan_y = excluded.camera_pan_y,
                    camera_zoom = excluded.camera_zoom,
                    auto_image_scaling = excluded.auto_image_scaling,
                    placement_gap = excluded.placement_gap,
                    import_target_canvas_area = excluded.import_target_canvas_area,
                    canvas_background = excluded.canvas_background,
                    semantic_search_enabled = excluded.semantic_search_enabled,
                    canvas_image_mode = excluded.canvas_image_mode,
                    canvas_image_max_side = excluded.canvas_image_max_side,
                    rotation_snap_degrees = excluded.rotation_snap_degrees,
                    default_textbox_background = excluded.default_textbox_background,
                    sidebar_open = excluded.sidebar_open,
                    sidebar_tab = excluded.sidebar_tab,
                    session_payload = excluded.session_payload,
                    active_board_id = excluded.active_board_id",
                params![
                    session.viewport.camera_pan_x,
                    session.viewport.camera_pan_y,
                    session.viewport.camera_zoom,
                    if session.settings.auto_image_scaling {
                        1_i64
                    } else {
                        0_i64
                    },
                    session.settings.placement_gap,
                    session.settings.import_target_canvas_area,
                    session.viewport.canvas_background.as_str(),
                    if session.settings.semantic_search_enabled {
                        1_i64
                    } else {
                        0_i64
                    },
                    "downscale",
                    i64::from(session.settings.canvas_image_max_side),
                    session.settings.rotation_snap_degrees,
                    if session.settings.default_textbox_background {
                        1_i64
                    } else {
                        0_i64
                    },
                    if session.sidebar_open { 1_i64 } else { 0_i64 },
                    session.sidebar_tab.as_str(),
                    session_payload,
                    active_board_id,
                ],
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

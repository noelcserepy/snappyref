//! Semantic search glue: service lifecycle, install prompts, and the
//! search overlay.

use eframe::egui;
use std::time::Instant;

use crate::search::semantic::default_semantic_search_asset_descriptor;
use crate::ui::sidebar::{SemanticSearchSettingsAction, SemanticSearchSettingsStatus};

use super::SnappyRefApp;
use super::dialogs::{Dialog, DialogAction, DialogChoice};

impl SnappyRefApp {
    /// The install state the Semantic Search settings section shows.
    pub(super) fn semantic_search_settings_status(&self) -> SemanticSearchSettingsStatus {
        use crate::search::semantic::LocalModelStoreStatus;
        match self.search.local_model_store_status() {
            LocalModelStoreStatus::Installed => SemanticSearchSettingsStatus::Installed,
            LocalModelStoreStatus::Missing => SemanticSearchSettingsStatus::Missing,
            LocalModelStoreStatus::Downloading { percent } => {
                SemanticSearchSettingsStatus::Downloading { percent }
            }
            LocalModelStoreStatus::Failed(_) => SemanticSearchSettingsStatus::Failed,
        }
    }

    pub(super) fn handle_semantic_search_settings_action(
        &mut self,
        action: Option<SemanticSearchSettingsAction>,
        ctx: &egui::Context,
    ) {
        match action {
            Some(SemanticSearchSettingsAction::Install)
            | Some(SemanticSearchSettingsAction::Retry) => {
                self.search.start_semantic_search_install(&mut self.status);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
            Some(SemanticSearchSettingsAction::Cancel) => {
                self.search.cancel_semantic_search_install(&mut self.status);
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            }
            Some(SemanticSearchSettingsAction::RequestRemove) => {
                self.pending_semantic_search_remove = true;
            }
            None => {}
        }
    }

    pub(super) fn drive_semantic_search_install(&mut self, ctx: &egui::Context) {
        if self.search.drive_semantic_search_install(&mut self.status) {
            if self.search.local_model_assets_available() {
                self.search.sync_service(
                    self.library.as_ref(),
                    &mut self.status,
                    self.settings.canvas_image_max_side,
                );
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    pub(super) fn should_show_semantic_search_first_launch_prompt(&self) -> bool {
        self.library.is_some()
            && !self.settings.semantic_search_first_launch_prompt_seen
            && self.search.local_model_needs_install()
    }

    pub(super) fn mark_semantic_search_first_launch_prompt_seen(&mut self) {
        self.settings.semantic_search_first_launch_prompt_seen = true;
        self.save_user_session();
    }

    pub(super) fn enable_semantic_search_from_first_launch_prompt(&mut self, ctx: &egui::Context) {
        self.mark_semantic_search_first_launch_prompt_seen();
        self.search.start_semantic_search_install(&mut self.status);
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    pub(super) fn show_semantic_search_first_launch_prompt(&mut self, ctx: &egui::Context) {
        if !self.should_show_semantic_search_first_launch_prompt() {
            return;
        }
        let detail = format!(
            "Find references by describing them. It needs a one-time {} MB download. You can also turn it on later in Settings or from search with {}.",
            default_semantic_search_asset_descriptor().megabytes(),
            crate::ui::shortcut::text(super::shortcuts::SEARCH_IMAGES),
        );
        let choice = Dialog {
            id: "enable-semantic-search",
            title: "Enable Semantic Search?",
            detail: &self.ai_request_note(&detail),
            cancel: "Not now",
            action: DialogAction::Primary("Enable"),
        }
        .show(ctx);
        match choice {
            Some(DialogChoice::Confirm) => {
                self.enable_semantic_search_from_first_launch_prompt(ctx)
            }
            Some(DialogChoice::Cancel) => self.mark_semantic_search_first_launch_prompt_seen(),
            None => {}
        }
    }

    pub(super) fn show_semantic_search_remove_confirmation(&mut self, ctx: &egui::Context) {
        if !self.pending_semantic_search_remove {
            return;
        }
        let detail = format!(
            "Deletes the {} MB search model from this computer. Your library and its search index stay.",
            default_semantic_search_asset_descriptor().megabytes(),
        );
        let choice = Dialog {
            id: "remove-semantic-search",
            title: "Remove Semantic Search?",
            detail: &self.ai_request_note(&detail),
            cancel: "Cancel",
            action: DialogAction::Destructive("Remove"),
        }
        .show(ctx);
        match choice {
            Some(DialogChoice::Confirm) => {
                self.pending_semantic_search_remove = false;
                self.search
                    .remove_semantic_search_assets_from_settings(&mut self.status);
            }
            Some(DialogChoice::Cancel) => self.pending_semantic_search_remove = false,
            None => {}
        }
    }

    pub(super) fn drive_embedding_backfill(&mut self, ctx: &egui::Context) {
        let idle = crate::search::EmbeddingBackfillIdleState {
            app_idle: self.frame_perf.import_results_drained == 0
                && !self.canvas.has_pending_crop_work()
                && !self.canvas.has_pending_image_loads()
                && !self.residency.has_pending_backfills(),
        };
        let now = Instant::now();
        if let Err(error) = self.search.drive_embedding_backfill(now, idle) {
            self.status = error;
        }
        if let Some(delay) = self.search.embedding_backfill_repaint_after(now) {
            ctx.request_repaint_after(delay);
        }
    }

    pub(super) fn show_search_overlay(&mut self, ctx: &egui::Context, screen_rect: egui::Rect) {
        let visible_canvas_rect = self.visible_canvas_screen_rect(screen_rect);
        if let Some(drop_request) = self.search.show_overlay(
            ctx,
            screen_rect,
            &mut self.canvas,
            &mut self.status,
            crate::search::SearchPlacementCreationConfig {
                placement_target_area: self.settings.placement_target_area,
                placement_gap: self.settings.placement_gap,
                visible_canvas_rect,
            },
        ) {
            self.handle_search_drop(ctx, drop_request);
        }
        if self.search.take_recent_searches_dirty() {
            self.save_user_session();
        }
    }
}

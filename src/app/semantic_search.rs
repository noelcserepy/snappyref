//! Semantic search glue: service lifecycle, install prompts, and the
//! search overlay.

use eframe::egui;
use std::time::Instant;

use crate::ui::sidebar::{SemanticSearchSettingsAction, SemanticSearchSettingsStatus};

use super::SnappyRefApp;
use super::dialogs::{
    DialogChoice, destructive_dialog_width, dialog_actions, dialog_detail, dialog_frame,
    dialog_lead, dialog_title, semantic_search_prompt_width,
};

impl SnappyRefApp {
    /// The install state the Search settings page shows.
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
            && self.search.local_model_assets_missing()
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
        let entrance = crate::ui::motion::overlay_entrance(
            ctx,
            egui::Id::new("semantic_first_launch_prompt_entrance"),
            self.should_show_semantic_search_first_launch_prompt(),
        );
        if !self.should_show_semantic_search_first_launch_prompt() {
            return;
        }

        let megabytes =
            crate::search::semantic::default_semantic_search_asset_descriptor().megabytes();
        let mut choice = None;
        let settle = crate::ui::motion::settle_offset(entrance);
        egui::Window::new("Enable Semantic Search?")
            .id(egui::Id::new("semantic_search_first_launch_prompt"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, settle))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(dialog_frame(entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_width(semantic_search_prompt_width());
                ui.spacing_mut().item_spacing = crate::ui::card::ITEM_SPACING;
                dialog_title(ui, "Enable Semantic Search?");
                dialog_lead(
                    ui,
                    "Semantic Search can find references by description after a local download.",
                );
                dialog_detail(
                    ui,
                    &format!(
                        "Download {megabytes} MB of search assets now, or install later from Settings or the search palette with Command+F / Control+F."
                    ),
                );
                choice = dialog_actions(ui, "Not Now", |ui| {
                    crate::ui::card::button(ui, "Enable", 0.0)
                });
            });

        match choice {
            Some(DialogChoice::Confirm) => {
                self.enable_semantic_search_from_first_launch_prompt(ctx)
            }
            Some(DialogChoice::Cancel) => self.mark_semantic_search_first_launch_prompt_seen(),
            None => {}
        }
    }

    pub(super) fn show_semantic_search_remove_confirmation(&mut self, ctx: &egui::Context) {
        let entrance = crate::ui::motion::overlay_entrance(
            ctx,
            egui::Id::new("semantic_remove_dialog_entrance"),
            self.pending_semantic_search_remove,
        );
        if !self.pending_semantic_search_remove {
            return;
        }

        let settle = crate::ui::motion::settle_offset(entrance);
        egui::Window::new("Remove Semantic Search?")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, settle))
            .frame(dialog_frame(entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_width(destructive_dialog_width());
                ui.spacing_mut().item_spacing = crate::ui::card::ITEM_SPACING;
                dialog_title(ui, "Remove Semantic Search?");
                dialog_detail(
                    ui,
                    "Local search assets will be removed. Library data and saved search indexing stay intact.",
                );
                match dialog_actions(ui, "Cancel", |ui| {
                    crate::ui::card::destructive_button(ui, "Remove Semantic Search", 0.0)
                }) {
                    Some(DialogChoice::Confirm) => {
                        self.pending_semantic_search_remove = false;
                        self.search
                            .remove_semantic_search_assets_from_settings(&mut self.status);
                    }
                    Some(DialogChoice::Cancel) => self.pending_semantic_search_remove = false,
                    None => {}
                }
            });
    }

    pub(super) fn drive_embedding_backfill(&mut self, ctx: &egui::Context) {
        let idle = crate::search::EmbeddingBackfillIdleState {
            app_idle: self.frame_perf.import_results_drained == 0
                && !self.canvas.has_pending_crop_work()
                && !self.canvas.has_pending_image_loads()
                && !self.residency.has_pending_backfills(),
        };
        if let Err(error) = self.search.drive_embedding_backfill(Instant::now(), idle) {
            self.status = error;
        }
        if self.search.embedding_backfill_may_need_repaint() {
            ctx.request_repaint_after(std::time::Duration::from_millis(750));
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

//! Semantic search glue: service lifecycle, install prompts, and the
//! search overlay.

use eframe::egui;
use std::time::Instant;

use crate::ui::sidebar::{SemanticSearchSettingsAction, SemanticSearchSettingsStatus};

use super::SnappyRefApp;
use super::dialogs::{
    destructive_action_button, destructive_confirmation_frame, destructive_dialog_body,
    destructive_dialog_header, destructive_dialog_width, primary_dialog_button,
    secondary_dialog_button, semantic_search_prompt_body, semantic_search_prompt_frame,
    semantic_search_prompt_header, semantic_search_prompt_width,
};

pub(super) fn semantic_search_settings_status(
    status: crate::search::semantic::LocalModelStoreStatus,
) -> SemanticSearchSettingsStatus {
    match status {
        crate::search::semantic::LocalModelStoreStatus::Installed
        | crate::search::semantic::LocalModelStoreStatus::InstalledViaMigration => {
            SemanticSearchSettingsStatus::Installed
        }
        crate::search::semantic::LocalModelStoreStatus::Missing => {
            SemanticSearchSettingsStatus::Missing
        }
        crate::search::semantic::LocalModelStoreStatus::Downloading { percent } => {
            SemanticSearchSettingsStatus::Downloading { percent }
        }
        crate::search::semantic::LocalModelStoreStatus::Failed(_) => {
            SemanticSearchSettingsStatus::Failed
        }
    }
}

impl SnappyRefApp {
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

    pub(super) fn sync_semantic_search_service(&mut self) {
        self.search.sync_service(
            self.library.as_ref(),
            &mut self.status,
            Some(&mut self.canvas),
            self.settings.canvas_image_max_side,
        );
    }

    pub(super) fn handle_search_results(&mut self) {
        self.search.handle_results(&mut self.canvas);
    }

    pub(super) fn drive_semantic_search_install(&mut self, ctx: &egui::Context) {
        if self.search.drive_semantic_search_install(&mut self.status) {
            if self.search.local_model_assets_available() {
                self.sync_semantic_search_service();
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

    pub(super) fn decline_semantic_search_first_launch_prompt(&mut self) {
        self.mark_semantic_search_first_launch_prompt_seen();
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

        let mut decline = false;
        let mut enable = false;
        let settle = crate::ui::motion::settle_offset(entrance);
        egui::Window::new("Enable Semantic Search?")
            .id(egui::Id::new("semantic_search_first_launch_prompt"))
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, settle))
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(semantic_search_prompt_frame(ctx, entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_width(semantic_search_prompt_width());
                semantic_search_prompt_header(ui, "Enable Semantic Search?");
                semantic_search_prompt_body(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "Semantic Search can find references by description after a local download.",
                        )
                        .text_style(egui::TextStyle::Body)
                        .color(crate::ui::theme::TEXT),
                    );
                    ui.add_space(crate::ui::theme::SPACE_2);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Download about 200 MB of search assets now, or install later from Settings or the search palette with Command+F / Control+F.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::ui::theme::SPACE_4);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(primary_dialog_button("Enable")).clicked() {
                                enable = true;
                            }
                            if ui.add(secondary_dialog_button("Not Now")).clicked() {
                                decline = true;
                            }
                        });
                    });
                });
            });

        if enable {
            self.enable_semantic_search_from_first_launch_prompt(ctx);
        } else if decline {
            self.decline_semantic_search_first_launch_prompt();
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
            .frame(destructive_confirmation_frame(ctx, entrance))
            .show(ctx, |ui| {
                ui.multiply_opacity(entrance);
                ui.set_min_width(destructive_dialog_width());
                destructive_dialog_header(ui, "Remove Semantic Search?");
                destructive_dialog_body(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(
                                "Local search assets will be removed. Library data and saved search indexing stay intact.",
                            )
                            .text_style(egui::TextStyle::Small)
                            .color(crate::ui::theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                    ui.add_space(crate::ui::theme::SPACE_3);
                    ui.horizontal(|ui| {
                        if ui.add(secondary_dialog_button("Cancel")).clicked() {
                            self.pending_semantic_search_remove = false;
                        }
                        if ui
                            .add(destructive_action_button("Remove Semantic Search"))
                            .clicked()
                        {
                            self.pending_semantic_search_remove = false;
                            self.search
                                .remove_semantic_search_assets_from_settings(&mut self.status);
                        }
                    });
                });
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

    pub(super) fn handle_search_shortcut(&mut self, ctx: &egui::Context) {
        self.search.handle_shortcut(
            ctx,
            self.library.as_ref(),
            &mut self.canvas,
            &mut self.status,
            self.settings.canvas_image_max_side,
        );
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
            self.handle_search_drop(ctx, drop_request, screen_rect);
        }
        if self.search.take_recent_searches_dirty() {
            self.save_user_session();
        }
    }

    pub(super) fn search_using_selected_image(&mut self) {
        self.search.search_using_selected_image(
            self.library.as_ref(),
            &mut self.canvas,
            &mut self.status,
            self.settings.canvas_image_max_side,
        );
    }
}

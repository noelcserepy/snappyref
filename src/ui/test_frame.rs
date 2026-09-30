use eframe::egui;

/// Runs one egui frame without a renderer. Tests never upload textures, so the
/// frame's texture deltas are dropped here instead of tripping epaint's
/// unapplied-delta assertion.
pub(crate) fn run_ui(
    ctx: &egui::Context,
    input: egui::RawInput,
    ui: impl FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, ui);
    output.textures_delta.clear();
    output
}

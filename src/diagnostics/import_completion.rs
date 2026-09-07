//! Optional import event construction. Document work completes before reporting.
use crate::placement::content_resolution::LoadedContentResolutionOutcome;
pub(crate) fn record_loaded_import(
    mut scenario: Option<&mut crate::app::perf_scenario::PerfScenarioState>,
    perf_log: Option<&mut crate::app::perf_scenario::PerfLogState>,
    outcome: &LoadedContentResolutionOutcome,
) {
    if let Some(scenario) = &mut scenario
        && scenario.name == "startup"
        && let Some(log) = &outcome.resolved
        && log.origin == crate::image_preparation::ImportOrigin::Existing
        && scenario
            .startup_loaded_placement_ids
            .insert(log.placement.id())
    {
        // Count Placements once, including when several LOD results land.
        scenario.loaded_imports = scenario.startup_loaded_placement_ids.len();
        if !scenario.first_loaded_logged {
            scenario.first_loaded_logged = true;
            scenario.emit("first_placement_loaded", serde_json::json!({}));
        }
        if scenario.loaded_imports == scenario.expected_imports {
            scenario.all_loaded_logged = true;
            scenario.emit("all_placements_loaded", serde_json::json!({}));
        }
    }
    let resolution_queue_id = outcome.resolution_ticket.map(|ticket| ticket.queue_id);
    let perf_import_origin = outcome.resolved.as_ref().map(|log| log.origin);
    let perf_counts_as_import = perf_import_origin.is_some_and(|origin| {
        matches!(
            origin,
            crate::image_preparation::ImportOrigin::Path
                | crate::image_preparation::ImportOrigin::Clipboard
                | crate::image_preparation::ImportOrigin::Url
        )
    });
    let indexed_asset = &outcome.indexed_asset;
    if let Some(log) = &outcome.resolved {
        if perf_counts_as_import && let Some(scenario) = &mut scenario {
            scenario.loaded_imports += 1;
            let metadata = resolution_queue_id
                .and_then(|queue_id| scenario.asset_by_queue_id.get(&queue_id).cloned());
            if let Some(metadata) = &metadata {
                scenario
                    .asset_by_asset_id
                    .insert(log.placement.require_loaded_asset().id, metadata.clone());
            }
            let (format_value, source_file, animated, bytes) = match &metadata {
                Some(meta) => (
                    Some(meta.format.clone()),
                    Some(meta.file.clone()),
                    Some(meta.animated),
                    meta.bytes,
                ),
                None => (None, None, None, None),
            };
            if !scenario.first_loaded_logged {
                scenario.first_loaded_logged = true;
                scenario.emit(
                    "first_placement_loaded",
                    serde_json::json!({
                        "format": format_value,
                        "file": source_file,
                    }),
                );
            }
            scenario.emit(
                "placement_loaded",
                serde_json::json!({
                    "loaded": scenario.loaded_imports,
                    "asset_id": log.placement.require_loaded_asset().id,
                    "placement_id": log.placement.state.id,
                    "mime_type": log.placement.require_loaded_asset().mime_type,
                    "filename": log.placement.require_loaded_asset().filename,
                    "format": format_value,
                    "file": source_file,
                    "animated": animated,
                    "bytes": bytes,
                    "queue_id": resolution_queue_id,
                }),
            );
            if scenario.loaded_imports >= scenario.expected_imports && !scenario.all_loaded_logged {
                scenario.all_loaded_logged = true;
                scenario.emit("all_placements_loaded", serde_json::json!({}));
            }
        }
        log_loaded_timing(
            perf_log,
            log.origin,
            Some(&log.placement),
            log.diagnostics.as_ref(),
            log.trace,
        );
    }
    if let Some((asset_id, _original_path)) = indexed_asset {
        if perf_counts_as_import
            && let Some(scenario) = &mut scenario
            && scenario.pending_embedding_asset_ids.insert(*asset_id)
        {
            scenario.demand_embeddings_queued += 1;
            let metadata = scenario.asset_by_asset_id.get(asset_id).cloned();
            let (format_value, source_file) = match &metadata {
                Some(meta) => (Some(meta.format.clone()), Some(meta.file.clone())),
                None => (None, None),
            };
            if !scenario.first_embedding_queued_logged {
                scenario.first_embedding_queued_logged = true;
                scenario.emit(
                    "first_import_embedding_queued",
                    serde_json::json!({
                        "format": format_value,
                        "file": source_file,
                    }),
                );
            }
            scenario.emit(
                "import_embedding_queued",
                serde_json::json!({
                    "asset_id": asset_id,
                    "format": format_value,
                    "file": source_file,
                }),
            );
        }
    }
}
pub(crate) fn log_loaded_timing(
    perf_log: Option<&mut crate::app::perf_scenario::PerfLogState>,
    origin: crate::image_preparation::ImportOrigin,
    placement: Option<&crate::data::Placement>,
    diagnostics: Option<&crate::image_preparation::ExistingLoadDiagnostics>,
    trace: Option<crate::image_preparation::ImagePrepTrace>,
) {
    let Some(perf_log) = perf_log else {
        return;
    };

    let format_trace = |trace: crate::image_preparation::ImagePrepTrace| {
        format!(
            " total_ms={:.2} queue_ms={:.2} decode_ms={:.2} store_ms={:.2} preview_ms={:.2} worker_ms={:.2} ui_ms={:.2}",
            trace.queued_at.elapsed().as_secs_f64() * 1000.0,
            trace.timings.queue_wait.as_secs_f64() * 1000.0,
            trace.timings.decode_or_load.as_secs_f64() * 1000.0,
            trace.timings.store_and_persist.as_secs_f64() * 1000.0,
            trace.timings.preview_prepare.as_secs_f64() * 1000.0,
            trace.timings.worker_total.as_secs_f64() * 1000.0,
            trace.timings.ui_integration.as_secs_f64() * 1000.0,
        )
    };

    match origin {
        crate::image_preparation::ImportOrigin::Existing if !perf_log.startup_logged => {
            perf_log.startup_logged = true;
            let startup_ms = perf_log.app_started_at.elapsed().as_secs_f64() * 1000.0;
            if let Some(trace) = trace {
                eprintln!(
                    "[perf] startup_first_image_ms={startup_ms:.2}{}",
                    format_trace(trace)
                );
            } else {
                eprintln!("[perf] startup_first_image_ms={startup_ms:.2}");
            }
        }
        crate::image_preparation::ImportOrigin::Existing => {
            let startup_ms = perf_log.app_started_at.elapsed().as_secs_f64() * 1000.0;
            let asset = placement.map(|placement| {
                format!(
                    " placement_id={} asset_id={} file={}",
                    placement.state.id,
                    placement.require_loaded_asset().id,
                    placement.require_loaded_asset().filename
                )
            });
            let source = diagnostics.map(|diagnostics| {
                    format!(
                        " source_kind={:?} source_mb={:.2} backfilled_cache={} source={} source_dims={}x{}",
                        diagnostics.source_kind,
                        diagnostics.source_file_size as f64 / 1024.0 / 1024.0,
                        diagnostics.backfilled_cache,
                        diagnostics.source_path.display(),
                        diagnostics.source_width,
                        diagnostics.source_height
                    )
                });
            let trace_text = trace.map(format_trace).unwrap_or_default();
            eprintln!(
                "[perf] startup_image_visible startup_ms={startup_ms:.2}{}{}{}{}",
                asset.unwrap_or_default(),
                source.unwrap_or_default(),
                if trace_text.is_empty() { "" } else { " " },
                trace_text
            );
        }
        crate::image_preparation::ImportOrigin::Clipboard => {
            if let Some(trace) = trace {
                eprintln!("[perf] clipboard_import{}", format_trace(trace));
            }
        }
        crate::image_preparation::ImportOrigin::Path => {
            if let Some(trace) = trace {
                eprintln!("[perf] dropped_file_import{}", format_trace(trace));
            }
        }
        crate::image_preparation::ImportOrigin::Url => {
            if let Some(trace) = trace {
                eprintln!("[perf] url_import{}", format_trace(trace));
            }
        }
    }
}
pub(crate) fn log_cropped_timing(
    enabled: bool,
    placement: &crate::data::Placement,
    ui_integration_ms: f64,
) {
    if !enabled {
        return;
    }

    eprintln!(
        "[perf] crop_integrated placement_id={} asset_id={} file={} ui_ms={:.2}",
        placement.state.id,
        placement.require_loaded_asset().id,
        placement.require_loaded_asset().filename,
        ui_integration_ms
    );
}

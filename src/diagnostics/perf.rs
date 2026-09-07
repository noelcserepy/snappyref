pub fn print_file_decode_report(path: &std::path::Path) -> Result<(), String> {
    let report = crate::image_preparation::decode_image_file_perf_report(path)?;
    let frames = report
        .animated_frame_count
        .map(|count| format!(" animated_frames={count}"))
        .unwrap_or_default();

    println!(
        "decode source={} format={} source_mb={:.2} dims={}x{} megapixels={:.2} rgba_mb={:.2} decode_ms={:.2} ms_per_mp={:.2}{}",
        report.source_path.display(),
        report.format_name,
        report.source_file_size as f64 / 1024.0 / 1024.0,
        report.width,
        report.height,
        report.megapixels,
        report.rgba_bytes as f64 / 1024.0 / 1024.0,
        report.decode_ms,
        report.ms_per_megapixel,
        frames,
    );

    Ok(())
}

const TARGET: &str = "ui";

#[tauri::command]
pub fn log_write(level: String, message: String, fields: Option<String>) {
    let fields = fields.unwrap_or_default();
    match level.as_str() {
        "trace" => tracing::trace!(target: TARGET, %fields, "{message}"),
        "debug" => tracing::debug!(target: TARGET, %fields, "{message}"),
        "warn" => tracing::warn!(target: TARGET, %fields, "{message}"),
        "error" | "fatal" => tracing::error!(target: TARGET, %fields, "{message}"),
        _ => tracing::info!(target: TARGET, %fields, "{message}"),
    }
}

#[tauri::command]
pub fn crash_report(
    kind: String,
    message: String,
    location: String,
    detail: String,
) -> Option<String> {
    common::crash::record(&kind, &message, &location, &detail)
        .map(|path| path.display().to_string())
}

//! Safe mode: a run that loads no plugin stylesheet, for a theme that left the
//! app unusable. Asked for by `--safe-mode` or `HESTIA_SAFE_MODE=1`.

const ARG: &str = "--safe-mode";
const ENV: &str = "HESTIA_SAFE_MODE";

fn requested(args: impl IntoIterator<Item = String>, env: Option<&str>) -> bool {
    env == Some("1") || args.into_iter().any(|arg| arg == ARG)
}

#[tauri::command]
pub fn safe_mode() -> bool {
    let on = requested(std::env::args(), std::env::var(ENV).ok().as_deref());
    if on {
        tracing::warn!("safe mode: plugin themes are not loaded");
    }
    on
}

#[cfg(test)]
mod tests {
    use super::requested;

    #[test]
    fn the_flag_or_the_variable_turns_it_on() {
        assert!(requested(
            ["hestia-desktop".into(), "--safe-mode".into()],
            None
        ));
        assert!(requested([], Some("1")));
        assert!(!requested(["hestia-desktop".into()], Some("0")));
        assert!(!requested([], None));
    }
}

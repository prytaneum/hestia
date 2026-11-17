use sha2::{Digest, Sha256};

/// The digest the user approves for an install that brings other plugins: the
/// plugin's own digest and each planned plugin's, in install order. A plugin
/// that brings nothing is approved by its own digest alone.
pub fn plan_digest(own: &str, planned: &[&str]) -> String {
    if planned.is_empty() {
        return own.to_owned();
    }
    let mut hash = Sha256::new();
    let mut field = |text: &str| {
        hash.update((text.len() as u64).to_le_bytes());
        hash.update(text.as_bytes());
    };
    field("hestia-plan-v1");
    field(own);
    for digest in planned {
        field(digest);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

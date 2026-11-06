use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Error;

/// Reads a command's arguments into a struct. The daemon has already checked
/// them against the manifest, so an error here means the struct and the
/// declaration disagree.
///
/// ```
/// #[derive(serde::Deserialize)]
/// struct Args {
///     name: String,
/// }
///
/// let args: Args = hestia::args(serde_json::json!({ "name": "smp" }))?;
/// assert_eq!(args.name, "smp");
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn args<T: DeserializeOwned>(args: Value) -> Result<T, Error> {
    serde_json::from_value(args).map_err(|error| Error::Invalid(format!("arguments: {error}")))
}

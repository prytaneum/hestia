/// How many plugins one call path may pass through before it is refused, as a
/// backstop for loops that go through several plugins.
pub const MAX_CHAIN_DEPTH: usize = 8;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChainError {
    #[error("plugin `{0}` is already on this call path")]
    Reentry(String),
    #[error("call path is deeper than {MAX_CHAIN_DEPTH} plugins")]
    TooDeep,
}

/// The plugins on the current call path. It travels explicitly with the call,
/// never in a task-local: the job runner moves work into `tokio::spawn`, which
/// would drop a task-local and let a plugin loop through its own job.
#[derive(Debug, Clone, Default)]
pub struct CallChain(Vec<String>);

impl CallChain {
    /// The chain with `plugin` appended, or why it may not run on this path.
    pub fn enter(&self, plugin: &str) -> Result<CallChain, ChainError> {
        if self.0.iter().any(|on_path| on_path == plugin) {
            return Err(ChainError::Reentry(plugin.to_string()));
        }
        if self.0.len() >= MAX_CHAIN_DEPTH {
            return Err(ChainError::TooDeep);
        }
        let mut next = self.0.clone();
        next.push(plugin.to_string());
        Ok(CallChain(next))
    }
}

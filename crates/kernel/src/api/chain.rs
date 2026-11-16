const MAX_DEPTH: usize = 8;

/// The plugins a call has passed through. A plugin cannot appear twice and the
/// chain is at most [`MAX_DEPTH`] long, so extension points cannot loop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chain(Vec<String>);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChainError {
    #[error("plugin '{0}' is already in this call chain")]
    Reentered(String),
    #[error("a call chain is at most {MAX_DEPTH} plugins deep")]
    TooDeep,
}

impl Chain {
    pub fn enter(&self, plugin: &str) -> Result<Chain, ChainError> {
        if self.0.iter().any(|held| held == plugin) {
            return Err(ChainError::Reentered(plugin.to_owned()));
        }
        if self.0.len() >= MAX_DEPTH {
            return Err(ChainError::TooDeep);
        }
        let mut next = self.0.clone();
        next.push(plugin.to_owned());
        Ok(Chain(next))
    }

    pub fn plugins(&self) -> &[String] {
        &self.0
    }

    /// The plugin making the call: the last one entered.
    pub fn caller(&self) -> Option<&str> {
        self.0.last().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_never_holds_a_plugin_twice_nor_grows_past_its_depth() {
        let mut chain = Chain::default();
        for depth in 0..MAX_DEPTH {
            chain = chain.enter(&format!("p{depth}")).unwrap();
        }
        assert_eq!(chain.caller(), Some("p7"));
        assert_eq!(chain.enter("p9"), Err(ChainError::TooDeep));
        let short = Chain::default().enter("a").unwrap().enter("b").unwrap();
        assert_eq!(short.enter("a"), Err(ChainError::Reentered("a".into())));
    }
}

//! The entities one request resolved, so the `changed` event it publishes can
//! name them. A handler never builds the list: resolving an entry through the
//! guards records its stable id, because a caller may have named it by display
//! name and a rename changes the id.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct Changed(Arc<Mutex<BTreeSet<String>>>);

impl Changed {
    pub fn add(&self, id: &str) {
        self.0.lock().unwrap().insert(id.to_string());
    }

    pub fn ids(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_sorted() {
        let touched = Changed::default();
        touched.add("b");
        touched.add("a");
        touched.add("b");
        assert_eq!(touched.ids(), ["a", "b"]);
    }
}

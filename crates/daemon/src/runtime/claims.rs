//! The one lock an entry's tree has. Whatever is about to rewrite, read as a
//! whole or launch from an entry takes its claim first, and a second claim on
//! the same key is refused with the kind of job that holds it — so the refusal
//! names its holder and the check cannot race the claim.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// What holds a key. The guards derive the typed busy error from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    /// A start or restart spawning the process.
    Launch,
    /// Something being provisioned that no entry exists for yet: a server
    /// create, a Java runtime.
    Install,
    Update,
    Backup,
    /// Rewriting the content pool or game directory, as a job or inline
    /// (content, rename, remove, sync).
    Content,
    Modpack,
    Export,
}

/// The key a server create holds: its entry has no id until the engine makes
/// the record, so it is held by the requested name.
pub fn create_key(name: &str) -> String {
    format!("create-{name}")
}

#[derive(Clone, Default)]
pub struct Claims {
    held: Arc<Mutex<HashMap<String, JobKind>>>,
}

impl Claims {
    pub fn holder(&self, key: &str) -> Option<JobKind> {
        self.held.lock().unwrap().get(key).copied()
    }

    /// Take `key`, or learn what holds it. The claim releases on drop, so a job
    /// that panics never wedges its key.
    pub fn claim(&self, key: String, kind: JobKind) -> Result<Claim, JobKind> {
        let mut held = self.held.lock().unwrap();
        if let Some(holder) = held.get(&key) {
            return Err(*holder);
        }
        held.insert(key.clone(), kind);
        Ok(Claim {
            held: self.held.clone(),
            key,
        })
    }
}

pub struct Claim {
    held: Arc<Mutex<HashMap<String, JobKind>>>,
    key: String,
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.held.lock().unwrap().remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_admits_one_holder_and_names_it() {
        let claims = Claims::default();
        let _backup = claims.claim("server-a".into(), JobKind::Backup).unwrap();

        assert_eq!(
            claims.claim("server-a".into(), JobKind::Content).err(),
            Some(JobKind::Backup)
        );
        assert!(claims.claim("server-b".into(), JobKind::Content).is_ok());
    }

    #[test]
    fn dropping_a_claim_releases_its_key() {
        let claims = Claims::default();
        drop(claims.claim("server-a".into(), JobKind::Update).unwrap());

        assert_eq!(claims.holder("server-a"), None);
        assert!(claims.claim("server-a".into(), JobKind::Backup).is_ok());
    }
}

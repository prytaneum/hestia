use super::bindings::hestia::kernel::permissions;
use super::host::State;

impl permissions::Host for State {
    fn granted(&mut self, permission: String) -> bool {
        self.reach.granted.contains(&permission)
    }

    fn entries(&mut self) -> permissions::EntryScope {
        let scope = self.reach.entries.get();
        permissions::EntryScope {
            all: scope.all,
            ids: scope.ids,
        }
    }
}

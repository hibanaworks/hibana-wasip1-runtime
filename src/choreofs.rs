#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectId(pub u32);

/// Immutable path-to-object fact consumed by driver-side logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsFact<'a> {
    path: &'a [u8],
    object: ObjectId,
}

impl<'a> ChoreoFsFact<'a> {
    const EMPTY: Self = Self {
        path: &[],
        object: ObjectId(0),
    };

    pub const fn new(path: &'a [u8], object: ObjectId) -> Self {
        Self { path, object }
    }

    pub const fn path(&self) -> &'a [u8] {
        self.path
    }

    pub const fn object(&self) -> ObjectId {
        self.object
    }
}

/// Immutable fd materialization spec for one ChoreoFS object.
///
/// This helper is only shorthand for ledger facts. It does not own protocol
/// progress, route selection, or boundary authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdSpec {
    fd: u32,
    rights: u64,
    generation: u32,
}

impl FdSpec {
    pub const fn new(fd: u32, rights: u64, generation: u32) -> Self {
        Self {
            fd,
            rights,
            generation,
        }
    }

    pub const fn fd(&self) -> u32 {
        self.fd
    }

    pub const fn rights(&self) -> u64 {
        self.rights
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }
}

/// Const helper for writing ChoreoFS path/object and fd facts as one object.
///
/// `ChoreoFsObject` is not a manifest and not an authority table. It only expands
/// into [`ChoreoFsFact`] and [`LedgerFdFact`] for driver-local facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsObject {
    path: &'static [u8],
    object: ObjectId,
    fd: FdSpec,
}

impl ChoreoFsObject {
    pub const fn new(path: &'static [u8], object: ObjectId, fd: FdSpec) -> Self {
        Self { path, object, fd }
    }

    pub const fn path(&self) -> &'static [u8] {
        self.path
    }

    pub const fn object(&self) -> ObjectId {
        self.object
    }

    pub const fn fd(&self) -> FdSpec {
        self.fd
    }

    pub const fn choreofs_fact(&self) -> ChoreoFsFact<'static> {
        ChoreoFsFact::new(self.path, self.object)
    }

    pub const fn ledger_fd_fact(&self) -> LedgerFdFact {
        LedgerFdFact::new(self.fd.fd, self.object, self.fd.rights, self.fd.generation)
    }
}

/// Bounded static expansion of ChoreoFS object facts into driver facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsObjectSet<const N: usize> {
    choreofs: [ChoreoFsFact<'static>; N],
    ledger: [LedgerFdFact; N],
}

impl<const N: usize> ChoreoFsObjectSet<N> {
    pub const fn new(specs: [ChoreoFsObject; N]) -> Self {
        let mut choreofs = [ChoreoFsFact::EMPTY; N];
        let mut ledger = [LedgerFdFact::EMPTY; N];
        let mut idx = 0usize;
        while idx < N {
            choreofs[idx] = specs[idx].choreofs_fact();
            ledger[idx] = specs[idx].ledger_fd_fact();
            idx += 1;
        }
        Self { choreofs, ledger }
    }

    pub const fn choreofs_facts(&'static self) -> ChoreoFsFacts<'static> {
        ChoreoFsFacts::new(&self.choreofs)
    }

    pub const fn ledger_facts(&'static self) -> LedgerFacts<'static> {
        LedgerFacts::new(&self.ledger)
    }

    pub const fn driver_facts(&'static self) -> DriverFacts<'static> {
        DriverFacts::new(self.choreofs_facts(), self.ledger_facts())
    }
}

/// ChoreoFS fact resolver. It does not own protocol progress or route authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsFacts<'a> {
    entries: &'a [ChoreoFsFact<'a>],
}

impl<'a> ChoreoFsFacts<'a> {
    pub const fn new(entries: &'a [ChoreoFsFact<'a>]) -> Self {
        Self { entries }
    }

    pub const fn entries(&self) -> &'a [ChoreoFsFact<'a>] {
        self.entries
    }

    pub fn resolve(&self, path: &[u8]) -> Option<ObjectId> {
        let mut idx = 0usize;
        while idx < self.entries.len() {
            let entry = self.entries[idx];
            if entry.path == path {
                return Some(entry.object);
            }
            idx += 1;
        }
        None
    }
}

/// Immutable fd/object materialization fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerFdFact {
    fd: u32,
    object: ObjectId,
    rights: u64,
    generation: u32,
}

impl LedgerFdFact {
    const EMPTY: Self = Self {
        fd: 0,
        object: ObjectId(0),
        rights: 0,
        generation: 0,
    };

    pub const fn new(fd: u32, object: ObjectId, rights: u64, generation: u32) -> Self {
        Self {
            fd,
            object,
            rights,
            generation,
        }
    }

    pub const fn fd(&self) -> u32 {
        self.fd
    }

    pub const fn object(&self) -> ObjectId {
        self.object
    }

    pub const fn rights(&self) -> u64 {
        self.rights
    }

    pub const fn generation(&self) -> u32 {
        self.generation
    }
}

/// Read-only ledger facts. The choreography still owns progress authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerFacts<'a> {
    fds: &'a [LedgerFdFact],
}

impl<'a> LedgerFacts<'a> {
    pub const fn new(fds: &'a [LedgerFdFact]) -> Self {
        Self { fds }
    }

    pub const fn fds(&self) -> &'a [LedgerFdFact] {
        self.fds
    }

    pub fn fd(&self, fd: u32) -> Option<LedgerFdFact> {
        let mut idx = 0usize;
        while idx < self.fds.len() {
            let fact = self.fds[idx];
            if fact.fd == fd {
                return Some(fact);
            }
            idx += 1;
        }
        None
    }
}

/// Driver-side service facts handed to sealed localside contexts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriverFacts<'a> {
    choreofs: ChoreoFsFacts<'a>,
    ledger: LedgerFacts<'a>,
}

impl<'a> DriverFacts<'a> {
    pub const fn new(choreofs: ChoreoFsFacts<'a>, ledger: LedgerFacts<'a>) -> Self {
        Self { choreofs, ledger }
    }

    pub const fn choreofs(&self) -> ChoreoFsFacts<'a> {
        self.choreofs
    }

    pub const fn ledger(&self) -> LedgerFacts<'a> {
        self.ledger
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static OBJECTS: ChoreoFsObjectSet<2> = ChoreoFsObjectSet::new([
        ChoreoFsObject::new(b"device/led/green", ObjectId(1), FdSpec::new(10, 0b01, 100)),
        ChoreoFsObject::new(b"device/led/red", ObjectId(2), FdSpec::new(11, 0b10, 101)),
    ]);

    #[test]
    fn object_set_expands_choreofs_and_ledger_driver_facts() {
        let facts = OBJECTS.driver_facts();

        assert_eq!(facts.choreofs().entries().len(), 2);
        assert_eq!(
            facts.choreofs().resolve(b"device/led/green"),
            Some(ObjectId(1))
        );
        assert_eq!(
            facts.choreofs().resolve(b"device/led/red"),
            Some(ObjectId(2))
        );
        assert_eq!(facts.choreofs().resolve(b"device/led/yellow"), None);

        let green_fd = facts.ledger().fd(10).expect("green fd fact");
        assert_eq!(green_fd.object(), ObjectId(1));
        assert_eq!(green_fd.rights(), 0b01);
        assert_eq!(green_fd.generation(), 100);

        let red_fd = facts.ledger().fd(11).expect("red fd fact");
        assert_eq!(red_fd.object(), ObjectId(2));
        assert_eq!(red_fd.rights(), 0b10);
        assert_eq!(red_fd.generation(), 101);

        assert_eq!(facts.ledger().fd(12), None);
    }
}

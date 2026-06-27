use hibana::runtime::wire::CodecError;

use crate::protocol::{
    self, FdBinding, FdClosed, FdReadDone, FdReaddirDone, FdStat, FdWriteDone, MemRights,
    PathOpened,
};

const ERRNO_SUCCESS: u16 = 0;
const ERRNO_ACCES: u16 = 2;
const ERRNO_BADF: u16 = 8;
const ERRNO_NOENT: u16 = 44;

const FD_READ_RIGHT: u64 = 1 << 1;
const FD_WRITE_RIGHT: u64 = 1 << 6;
const FD_READDIR_RIGHT: u64 = 1 << 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectId(pub u32);

/// Immutable path-to-object fact consumed by local ChoreoFS object logic.
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
/// into [`ChoreoFsFact`] and [`LedgerFdFact`] for local object facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsObject {
    path: &'static [u8],
    object: ObjectId,
    fd: FdSpec,
    material: ChoreoFsObjectMaterial<'static>,
}

impl ChoreoFsObject {
    pub const fn new(path: &'static [u8], object: ObjectId, fd: FdSpec) -> Self {
        Self {
            path,
            object,
            fd,
            material: ChoreoFsObjectMaterial::empty(object),
        }
    }

    pub const fn readable(
        path: &'static [u8],
        object: ObjectId,
        fd: FdSpec,
        bytes: &'static [u8],
        binding: FdBinding,
    ) -> Self {
        Self {
            path,
            object,
            fd,
            material: ChoreoFsObjectMaterial::readable(object, bytes, binding),
        }
    }

    pub const fn readdir(
        path: &'static [u8],
        object: ObjectId,
        fd: FdSpec,
        listing: &'static [u8],
        binding: FdBinding,
    ) -> Self {
        Self {
            path,
            object,
            fd,
            material: ChoreoFsObjectMaterial::readdir(object, listing, binding),
        }
    }

    pub const fn writable(
        path: &'static [u8],
        object: ObjectId,
        fd: FdSpec,
        binding: FdBinding,
    ) -> Self {
        Self {
            path,
            object,
            fd,
            material: ChoreoFsObjectMaterial::writable(object, binding),
        }
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

    const fn material(&self) -> ChoreoFsObjectMaterial<'static> {
        self.material
    }
}

/// Bounded static expansion of ChoreoFS object facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsObjectSet<const N: usize> {
    choreofs: [ChoreoFsFact<'static>; N],
    ledger: [LedgerFdFact; N],
    materials: [ChoreoFsObjectMaterial<'static>; N],
}

impl<const N: usize> ChoreoFsObjectSet<N> {
    pub const fn new(specs: [ChoreoFsObject; N]) -> Self {
        let mut choreofs = [ChoreoFsFact::EMPTY; N];
        let mut ledger = [LedgerFdFact::EMPTY; N];
        let mut materials = [ChoreoFsObjectMaterial::EMPTY; N];
        let mut idx = 0usize;
        while idx < N {
            choreofs[idx] = specs[idx].choreofs_fact();
            ledger[idx] = specs[idx].ledger_fd_fact();
            materials[idx] = specs[idx].material();
            idx += 1;
        }
        Self {
            choreofs,
            ledger,
            materials,
        }
    }

    pub const fn choreofs_facts(&'static self) -> ChoreoFsFacts<'static> {
        ChoreoFsFacts::new(&self.choreofs)
    }

    pub const fn ledger_facts(&'static self) -> LedgerFacts<'static> {
        LedgerFacts::new(&self.ledger)
    }

    pub const fn choreofs(&'static self) -> ChoreoFs<'static> {
        ChoreoFs::new_with_materials(
            self.choreofs_facts(),
            self.ledger_facts(),
            ChoreoFsObjectMaterials::new(&self.materials),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChoreoFsObjectMaterial<'a> {
    object: ObjectId,
    binding: FdBinding,
    kind: ChoreoFsObjectMaterialKind<'a>,
}

impl<'a> ChoreoFsObjectMaterial<'a> {
    const EMPTY: Self = Self {
        object: ObjectId(0),
        binding: FdBinding::none(),
        kind: ChoreoFsObjectMaterialKind::Empty,
    };

    const fn empty(object: ObjectId) -> Self {
        Self {
            object,
            binding: FdBinding::none(),
            kind: ChoreoFsObjectMaterialKind::Empty,
        }
    }

    const fn readable(object: ObjectId, bytes: &'a [u8], binding: FdBinding) -> Self {
        Self {
            object,
            binding,
            kind: ChoreoFsObjectMaterialKind::Readable(bytes),
        }
    }

    const fn readdir(object: ObjectId, listing: &'a [u8], binding: FdBinding) -> Self {
        Self {
            object,
            binding,
            kind: ChoreoFsObjectMaterialKind::Readdir(listing),
        }
    }

    const fn writable(object: ObjectId, binding: FdBinding) -> Self {
        Self {
            object,
            binding,
            kind: ChoreoFsObjectMaterialKind::Writable,
        }
    }

    const fn object(&self) -> ObjectId {
        self.object
    }

    const fn binding(&self) -> FdBinding {
        self.binding
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChoreoFsObjectMaterialKind<'a> {
    Empty,
    Readable(&'a [u8]),
    Readdir(&'a [u8]),
    Writable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChoreoFsObjectMaterials<'a> {
    entries: &'a [ChoreoFsObjectMaterial<'a>],
}

impl<'a> ChoreoFsObjectMaterials<'a> {
    const fn empty() -> Self {
        Self { entries: &[] }
    }

    const fn new(entries: &'a [ChoreoFsObjectMaterial<'a>]) -> Self {
        Self { entries }
    }

    pub fn object(&self, object: ObjectId) -> Option<ChoreoFsObjectMaterial<'a>> {
        let mut idx = 0usize;
        while idx < self.entries.len() {
            let entry = self.entries[idx];
            if entry.object == object {
                return Some(entry);
            }
            idx += 1;
        }
        None
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

    pub fn object_with_rights(&self, object: ObjectId, rights: u64) -> Option<LedgerFdFact> {
        let mut idx = 0usize;
        while idx < self.fds.len() {
            let fact = self.fds[idx];
            if fact.object == object && fact.rights & rights == rights {
                return Some(fact);
            }
            idx += 1;
        }
        None
    }
}

/// ChoreoFS object facts and fd ledger.
///
/// This type never owns endpoint progress. It only turns an already admitted
/// WASI request into a typed object operation token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFs<'a> {
    choreofs: ChoreoFsFacts<'a>,
    ledger: LedgerFacts<'a>,
    materials: ChoreoFsObjectMaterials<'a>,
}

impl<'a> ChoreoFs<'a> {
    pub const fn new(choreofs: ChoreoFsFacts<'a>, ledger: LedgerFacts<'a>) -> Self {
        Self {
            choreofs,
            ledger,
            materials: ChoreoFsObjectMaterials::empty(),
        }
    }

    const fn new_with_materials(
        choreofs: ChoreoFsFacts<'a>,
        ledger: LedgerFacts<'a>,
        materials: ChoreoFsObjectMaterials<'a>,
    ) -> Self {
        Self {
            choreofs,
            ledger,
            materials,
        }
    }

    pub const fn facts(&self) -> ChoreoFsFacts<'a> {
        self.choreofs
    }

    pub const fn ledger(&self) -> LedgerFacts<'a> {
        self.ledger
    }

    pub fn path_open(&self, open: protocol::PathOpen) -> ChoreoFsOpen {
        let Some(object) = self.choreofs.resolve(open.path()) else {
            return ChoreoFsOpen::denied(open, None, ERRNO_NOENT);
        };
        let Some(material) = self.materials.object(object) else {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_NOENT);
        };
        let Some(required_rights) = material_required_rights(material, open.rights_base()) else {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_ACCES);
        };
        if !binding_allows_rights(material.binding(), required_rights) {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_ACCES);
        }
        let Some(fact) = self.ledger.object_with_rights(object, required_rights) else {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_ACCES);
        };
        ChoreoFsOpen::opened(open, object, fact.fd() as u8, material.binding())
    }

    pub fn fd_readdir(&self, read: protocol::FdReaddir) -> ChoreoFsReadDir<'a> {
        let Some(material) = self.material_for_fd(read.fd()) else {
            return ChoreoFsReadDir::denied(read, None, ERRNO_BADF);
        };
        let ChoreoFsObjectMaterialKind::Readdir(listing) = material.kind else {
            return ChoreoFsReadDir::denied(read, Some(material.object()), ERRNO_BADF);
        };
        ChoreoFsReadDir::ready(read, material.object(), listing)
    }

    pub fn fd_read(&self, read: protocol::FdRead) -> ChoreoFsRead<'a> {
        let Some(material) = self.material_for_fd(read.fd()) else {
            return ChoreoFsRead::denied(read, None, ERRNO_BADF);
        };
        let ChoreoFsObjectMaterialKind::Readable(bytes) = material.kind else {
            return ChoreoFsRead::denied(read, Some(material.object()), ERRNO_BADF);
        };
        ChoreoFsRead::ready(read, material.object(), bytes)
    }

    pub fn fd_write(&self, write: protocol::FdWrite) -> ChoreoFsWrite {
        match self
            .material_for_fd(write.fd())
            .map(|material| (material.object(), material.kind))
        {
            Some((object, ChoreoFsObjectMaterialKind::Writable)) => {
                ChoreoFsWrite::ready(write, object)
            }
            Some((object, _)) => ChoreoFsWrite::denied(write, Some(object), ERRNO_ACCES),
            None => ChoreoFsWrite::denied(write, None, ERRNO_ACCES),
        }
    }

    pub fn fd_fdstat_get(&self, request: protocol::FdRequest) -> protocol::FdStatRet {
        let fd = request.fd();
        match self.material_for_fd(fd) {
            Some(material) => {
                protocol::FdStatRet(FdStat::new(fd, rights_from_binding(material.binding())))
            }
            None => protocol::FdStatRet(FdStat::new_with_errno(fd, MemRights::Read, ERRNO_BADF)),
        }
    }

    pub fn fd_close(&self, request: protocol::FdRequest) -> protocol::FdClosedRet {
        let fd = request.fd();
        let errno = if self.material_for_fd(fd).is_some() {
            ERRNO_SUCCESS
        } else {
            ERRNO_BADF
        };
        protocol::FdClosedRet(FdClosed::new_with_errno(fd, errno))
    }

    fn material_for_fd(&self, fd: u8) -> Option<ChoreoFsObjectMaterial<'a>> {
        self.ledger
            .fd(fd as u32)
            .and_then(|fact| self.materials.object(fact.object()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsOpen {
    request: protocol::PathOpen,
    object: Option<ObjectId>,
    fd: u8,
    binding: FdBinding,
    errno: u16,
}

impl ChoreoFsOpen {
    const fn opened(
        request: protocol::PathOpen,
        object: ObjectId,
        fd: u8,
        binding: FdBinding,
    ) -> Self {
        Self {
            request,
            object: Some(object),
            fd,
            binding,
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::PathOpen, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            request,
            object,
            fd: 0,
            binding: FdBinding::none(),
            errno,
        }
    }

    pub const fn request(&self) -> protocol::PathOpen {
        self.request
    }

    pub const fn object(&self) -> Option<ObjectId> {
        self.object
    }

    pub const fn fd(&self) -> Option<u8> {
        if self.errno == ERRNO_SUCCESS {
            Some(self.fd)
        } else {
            None
        }
    }

    pub const fn binding(&self) -> FdBinding {
        self.binding
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub const fn is_open(&self) -> bool {
        self.errno == ERRNO_SUCCESS
    }

    pub const fn opened_ret(self) -> protocol::PathOpenedRet {
        protocol::PathOpenedRet(PathOpened::new_with_binding(
            self.fd,
            self.errno,
            self.binding,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsRead<'a> {
    request: protocol::FdRead,
    object: Option<ObjectId>,
    bytes: Option<&'a [u8]>,
    errno: u16,
}

impl<'a> ChoreoFsRead<'a> {
    const fn ready(request: protocol::FdRead, object: ObjectId, bytes: &'a [u8]) -> Self {
        Self {
            request,
            object: Some(object),
            bytes: Some(bytes),
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::FdRead, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            request,
            object,
            bytes: None,
            errno,
        }
    }

    pub const fn request(&self) -> protocol::FdRead {
        self.request
    }

    pub const fn object(&self) -> Option<ObjectId> {
        self.object
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub fn read_from(self, offset: usize) -> Result<(protocol::FdReadDoneRet, usize), CodecError> {
        let Some(bytes) = self.bytes else {
            return Ok((
                protocol::FdReadDoneRet(FdReadDone::new_with_errno(
                    self.request.fd(),
                    b"",
                    self.errno,
                )?),
                offset,
            ));
        };
        let start = offset.min(bytes.len());
        let len = bytes[start..].len().min(self.request.max_len() as usize);
        Ok((
            protocol::FdReadDoneRet(FdReadDone::new(
                self.request.fd(),
                &bytes[start..start + len],
            )?),
            start + len,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsReadDir<'a> {
    request: protocol::FdReaddir,
    object: Option<ObjectId>,
    listing: Option<&'a [u8]>,
    errno: u16,
}

impl<'a> ChoreoFsReadDir<'a> {
    const fn ready(request: protocol::FdReaddir, object: ObjectId, listing: &'a [u8]) -> Self {
        Self {
            request,
            object: Some(object),
            listing: Some(listing),
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::FdReaddir, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            request,
            object,
            listing: None,
            errno,
        }
    }

    pub const fn request(&self) -> protocol::FdReaddir {
        self.request
    }

    pub const fn object(&self) -> Option<ObjectId> {
        self.object
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub fn read_dir(self) -> Result<protocol::FdReaddirDoneRet, CodecError> {
        let Some(listing) = self.listing else {
            return Ok(protocol::FdReaddirDoneRet(FdReaddirDone::new(
                self.request.fd(),
                b"",
                self.errno,
            )?));
        };
        let start = match usize::try_from(self.request.cookie()) {
            Ok(value) => value,
            Err(_) => usize::MAX,
        };
        let bytes = match listing.get(start..) {
            Some(bytes) => bytes,
            None => &[],
        };
        let len = bytes.len().min(self.request.max_len() as usize);
        Ok(protocol::FdReaddirDoneRet(FdReaddirDone::new(
            self.request.fd(),
            &bytes[..len],
            self.errno,
        )?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsWrite {
    request: protocol::FdWrite,
    object: Option<ObjectId>,
    errno: u16,
}

impl ChoreoFsWrite {
    const fn ready(request: protocol::FdWrite, object: ObjectId) -> Self {
        Self {
            request,
            object: Some(object),
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::FdWrite, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            request,
            object,
            errno,
        }
    }

    pub const fn request(&self) -> protocol::FdWrite {
        self.request
    }

    pub const fn object(&self) -> Option<ObjectId> {
        self.object
    }

    pub fn bytes(&self) -> &[u8] {
        self.request.as_bytes()
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub const fn is_writable(&self) -> bool {
        self.errno == ERRNO_SUCCESS
    }

    pub fn written(self) -> protocol::FdWriteDoneRet {
        if self.errno == ERRNO_SUCCESS {
            protocol::FdWriteDoneRet(FdWriteDone::new(
                self.request.fd(),
                self.request.len() as u8,
            ))
        } else {
            protocol::FdWriteDoneRet(FdWriteDone::new_with_errno(
                self.request.fd(),
                0,
                self.errno,
            ))
        }
    }
}

fn binding_allows_rights(binding: FdBinding, rights: u64) -> bool {
    if rights & FD_WRITE_RIGHT != 0 && binding.write.is_none() {
        return false;
    }
    if rights & FD_READDIR_RIGHT != 0 && binding.readdir.is_none() {
        return false;
    }
    if rights & FD_READ_RIGHT != 0 && binding.read.is_none() {
        return false;
    }
    true
}

fn material_required_rights(material: ChoreoFsObjectMaterial<'_>, rights: u64) -> Option<u64> {
    if rights & FD_WRITE_RIGHT != 0 {
        return match material.kind {
            ChoreoFsObjectMaterialKind::Writable => Some(FD_WRITE_RIGHT),
            _ => None,
        };
    }
    match material.kind {
        ChoreoFsObjectMaterialKind::Empty => None,
        ChoreoFsObjectMaterialKind::Readable(_) if rights & FD_READ_RIGHT != 0 => {
            Some(FD_READ_RIGHT)
        }
        ChoreoFsObjectMaterialKind::Readdir(_) if rights & FD_READDIR_RIGHT != 0 => {
            Some(FD_READDIR_RIGHT)
        }
        ChoreoFsObjectMaterialKind::Writable => Some(FD_WRITE_RIGHT),
        _ if rights & (FD_READ_RIGHT | FD_WRITE_RIGHT | FD_READDIR_RIGHT) == 0 => Some(0),
        _ => None,
    }
}

const fn rights_from_binding(binding: FdBinding) -> MemRights {
    if binding.write.is_some() {
        MemRights::Write
    } else {
        MemRights::Read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static OBJECTS: ChoreoFsObjectSet<2> = ChoreoFsObjectSet::new([
        ChoreoFsObject::new(
            b"outputs/led/green",
            ObjectId(1),
            FdSpec::new(10, FD_WRITE_RIGHT, 100),
        ),
        ChoreoFsObject::new(b"outputs/led/red", ObjectId(2), FdSpec::new(11, 0b10, 101)),
    ]);

    static RUNTIME_OBJECTS: ChoreoFsObjectSet<3> = ChoreoFsObjectSet::new([
        ChoreoFsObject::readdir(
            b"objects",
            ObjectId(10),
            FdSpec::new(4, FD_READDIR_RIGHT, 0),
            b"log\n",
            FdBinding::readdir(protocol::FdReaddirRow::Base),
        ),
        ChoreoFsObject::readable(
            b"objects/log",
            ObjectId(11),
            FdSpec::new(5, FD_READ_RIGHT, 0),
            b"session=attached\n",
            FdBinding::read(protocol::FdReadRow::Base),
        ),
        ChoreoFsObject::writable(
            b"outputs/led/green",
            ObjectId(12),
            FdSpec::new(6, FD_WRITE_RIGHT, 0),
            FdBinding::write(protocol::FdWriteRow::Object),
        ),
    ]);

    #[test]
    fn object_set_expands_choreofs_and_ledger_facts() {
        let choreofs = OBJECTS.choreofs();

        assert_eq!(choreofs.facts().entries().len(), 2);
        assert_eq!(
            choreofs.facts().resolve(b"outputs/led/green"),
            Some(ObjectId(1))
        );
        assert_eq!(
            choreofs.facts().resolve(b"outputs/led/red"),
            Some(ObjectId(2))
        );
        assert_eq!(choreofs.facts().resolve(b"outputs/led/yellow"), None);

        let green_fd = choreofs.ledger().fd(10).expect("green fd fact");
        assert_eq!(green_fd.object(), ObjectId(1));
        assert_eq!(green_fd.rights(), FD_WRITE_RIGHT);
        assert_eq!(green_fd.generation(), 100);

        let red_fd = choreofs.ledger().fd(11).expect("red fd fact");
        assert_eq!(red_fd.object(), ObjectId(2));
        assert_eq!(red_fd.rights(), 0b10);
        assert_eq!(red_fd.generation(), 101);

        assert_eq!(choreofs.ledger().fd(12), None);
    }

    #[test]
    fn choreofs_builds_object_operation_tokens() {
        let choreofs = RUNTIME_OBJECTS.choreofs();

        let open = choreofs.path_open(
            protocol::PathOpen::new(3, FD_READDIR_RIGHT, b"objects").expect("path_open"),
        );
        assert_eq!(open.object(), Some(ObjectId(10)));
        assert_eq!(open.fd(), Some(4));
        assert!(open.is_open());
        let opened = open.opened_ret();
        assert_eq!(opened.0.fd(), 4);
        assert_eq!(opened.0.errno(), ERRNO_SUCCESS);
        assert_eq!(
            opened.0.binding().readdir,
            Some(protocol::FdReaddirRow::Base)
        );

        let inherited_dir_open = choreofs.path_open(
            protocol::PathOpen::new(3, FD_READ_RIGHT | FD_READDIR_RIGHT, b"objects")
                .expect("path_open"),
        );
        assert_eq!(inherited_dir_open.fd(), Some(4));

        let inherited_read_open = choreofs.path_open(
            protocol::PathOpen::new(3, FD_READ_RIGHT | FD_READDIR_RIGHT, b"objects/log")
                .expect("path_open"),
        );
        assert_eq!(inherited_read_open.fd(), Some(5));

        let readdir =
            choreofs.fd_readdir(protocol::FdReaddir::new(4, 0, 16).expect("fd_readdir request"));
        assert_eq!(readdir.object(), Some(ObjectId(10)));
        let listing = readdir.read_dir().expect("fd_readdir ret");
        assert_eq!(listing.0.as_bytes(), b"log\n");

        let read = protocol::FdRead::new(5, 30).expect("fd_read request");
        let read = choreofs.fd_read(read);
        assert_eq!(read.object(), Some(ObjectId(11)));
        let (chunk, next_offset) = read.read_from(0).expect("fd_read ret");
        assert_eq!(chunk.0.as_bytes(), b"session=attached\n");
        assert_eq!(next_offset, b"session=attached\n".len());

        let denied_read = choreofs.fd_read(protocol::FdRead::new(9, 30).expect("fd_read request"));
        assert_eq!(denied_read.object(), None);
        assert_eq!(denied_read.errno(), ERRNO_BADF);
        let (denied_read, _) = denied_read.read_from(0).expect("fd_read ret");
        assert_eq!(denied_read.0.errno(), ERRNO_BADF);

        let denied_stat = choreofs.fd_fdstat_get(protocol::FdRequest::new(9));
        assert_eq!(denied_stat.0.fd(), 9);
        assert_eq!(denied_stat.0.errno(), ERRNO_BADF);

        let denied_close = choreofs.fd_close(protocol::FdRequest::new(9));
        assert_eq!(denied_close.0.fd(), 9);
        assert_eq!(denied_close.0.errno(), ERRNO_BADF);

        let write = choreofs.fd_write(protocol::FdWrite::new(6, b"1").expect("fd_write request"));
        assert_eq!(write.object(), Some(ObjectId(12)));
        assert_eq!(write.bytes(), b"1");
        let written = write.written();
        assert_eq!(written.0.written(), 1);
        assert_eq!(written.0.errno(), ERRNO_SUCCESS);

        let denied = choreofs.path_open(
            protocol::PathOpen::new(3, FD_WRITE_RIGHT, b"objects/log").expect("path_open"),
        );
        assert_eq!(denied.object(), Some(ObjectId(11)));
        assert!(!denied.is_open());
        let denied = denied.opened_ret();
        assert_eq!(denied.0.errno(), ERRNO_ACCES);
    }
}

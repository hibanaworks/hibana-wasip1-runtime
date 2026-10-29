use hibana::runtime::wire::{CodecError, ReceivedPayload, WirePayload};

use crate::protocol::{
    self, FdBinding, FdClosed, FdReadDone, FdReaddirDone, FdStat, FdWriteDone, MemRights,
    PathOpened,
};

/// Decode the filesystem request carried by a received protocol payload.
///
/// Custom message types can implement this when they carry a WASI request
/// together with application data. ChoreoFS consumes the received payload;
/// constructing or decoding request data alone cannot invoke an operation.
pub trait ChoreoFsRequest: WirePayload {
    /// The operation-specific WASI request, such as [`protocol::FdCloseReq`].
    type Request;

    /// Extract the request from bytes already validated by this payload's codec.
    fn request(input: &ReceivedPayload<'_, Self>) -> Self::Request;
}

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
    fd: u8,
    rights: u64,
    generation: u32,
}

impl FdSpec {
    pub const fn new(fd: u8, rights: u64, generation: u32) -> Self {
        Self {
            fd,
            rights,
            generation,
        }
    }

    pub const fn fd(&self) -> u8 {
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
pub struct ChoreoFsObject<'a> {
    path: &'a [u8],
    object: ObjectId,
    fd: FdSpec,
    material: ChoreoFsObjectMaterial<'a>,
}

impl<'a> ChoreoFsObject<'a> {
    pub const fn new(path: &'a [u8], object: ObjectId, fd: FdSpec) -> Self {
        Self {
            path,
            object,
            fd,
            material: ChoreoFsObjectMaterial::empty(object),
        }
    }

    pub const fn readable(
        path: &'a [u8],
        object: ObjectId,
        fd: FdSpec,
        bytes: &'a [u8],
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
        path: &'a [u8],
        object: ObjectId,
        fd: FdSpec,
        listing: &'a [u8],
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
        path: &'a [u8],
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

    /// Borrowed file contents remain owned by the answering role. A read-only
    /// open receives only a read binding, even when this object supports writes.
    pub const fn read_write(
        path: &'a [u8],
        object: ObjectId,
        fd: FdSpec,
        bytes: &'a [u8],
        binding: FdBinding,
    ) -> Self {
        Self {
            path,
            object,
            fd,
            material: ChoreoFsObjectMaterial {
                object,
                binding,
                kind: ChoreoFsObjectMaterialKind::ReadWrite(bytes),
            },
        }
    }

    pub const fn path(&self) -> &'a [u8] {
        self.path
    }

    pub const fn object(&self) -> ObjectId {
        self.object
    }

    pub const fn fd(&self) -> FdSpec {
        self.fd
    }

    pub const fn choreofs_fact(&self) -> ChoreoFsFact<'a> {
        ChoreoFsFact::new(self.path, self.object)
    }

    pub const fn ledger_fd_fact(&self) -> LedgerFdFact {
        LedgerFdFact::new(self.fd.fd, self.object, self.fd.rights, self.fd.generation)
    }

    const fn material(&self) -> ChoreoFsObjectMaterial<'a> {
        self.material
    }
}

/// Bounded static expansion of ChoreoFS object facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoreoFsObjectSet<'a, const N: usize> {
    choreofs: [ChoreoFsFact<'a>; N],
    ledger: [LedgerFdFact; N],
    materials: [ChoreoFsObjectMaterial<'a>; N],
}

impl<'a, const N: usize> ChoreoFsObjectSet<'a, N> {
    pub const fn new(specs: [ChoreoFsObject<'a>; N]) -> Self {
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

    pub const fn choreofs_facts(&self) -> ChoreoFsFacts<'_> {
        ChoreoFsFacts::new(&self.choreofs)
    }

    pub const fn ledger_facts(&self) -> LedgerFacts<'_> {
        LedgerFacts::new(&self.ledger)
    }

    pub const fn choreofs(&self) -> ChoreoFs<'_> {
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
    ReadWrite(&'a [u8]),
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
    fd: u8,
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

    pub const fn new(fd: u8, object: ObjectId, rights: u64, generation: u32) -> Self {
        Self {
            fd,
            object,
            rights,
            generation,
        }
    }

    pub const fn fd(&self) -> u8 {
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

    pub fn fd(&self, fd: u8) -> Option<LedgerFdFact> {
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
/// WASI request into a typed object operation token. Pass the value returned by
/// `recv` directly to the corresponding operation, then send its result.
///
/// Locally constructed requests cannot invoke object operations:
///
/// ```compile_fail
/// use hibana_wasip1_runtime::{choreofs::ChoreoFs, protocol::PathOpen};
/// fn bypass(fs: &ChoreoFs<'_>) {
///     fs.path_open(PathOpen::new(3, 1 << 6, b"output").unwrap());
/// }
/// ```
///
/// ```compile_fail
/// use hibana_wasip1_runtime::{choreofs::ChoreoFs, protocol::FdWrite};
/// fn bypass(fs: &ChoreoFs<'_>) {
///     fs.fd_write(FdWrite::new(4, b"bytes").unwrap());
/// }
/// ```
///
/// Reading metadata does not authorize a close, even though both requests
/// carry the same fd encoding:
///
/// ```compile_fail
/// use hibana::runtime::wire::ReceivedPayload;
/// use hibana_wasip1_runtime::{choreofs::ChoreoFs, protocol::FdFdstatGetReq};
/// fn wrong_operation(fs: &ChoreoFs<'_>, request: ReceivedPayload<'_, FdFdstatGetReq>) {
///     fs.fd_close(request);
/// }
/// ```
///
/// An unfinished operation retains the endpoint borrow:
///
/// ```compile_fail
/// use hibana::Endpoint;
/// use hibana_wasip1_runtime::{choreofs::ChoreoFs, protocol};
/// async fn advance_early(io: &mut Endpoint<'_, 1>, fs: &ChoreoFs<'_>) {
///     let request = io.recv::<protocol::FdWriteReqMsg>().await.unwrap();
///     let write = fs.fd_write(request);
///     io.send::<protocol::FdWriteRetMsg>(&protocol::FdWriteDoneRet(
///         protocol::FdWriteDone::new(4, 0),
///     )).await.unwrap();
///     let _ = write.written();
/// }
/// ```
///
/// Each received request can materialize one operation:
///
/// ```compile_fail
/// use hibana::runtime::wire::ReceivedPayload;
/// use hibana_wasip1_runtime::{choreofs::ChoreoFs, protocol::FdWriteReq};
/// fn twice(fs: &ChoreoFs<'_>, request: ReceivedPayload<'_, FdWriteReq>) {
///     let first = fs.fd_write(request);
///     let second = fs.fd_write(request);
/// }
/// ```
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

    /// Execute the received `PathOpenReq` request against these object facts.
    pub fn path_open<'r, P>(&self, received: ReceivedPayload<'r, P>) -> ChoreoFsOpen<'r>
    where
        P: ChoreoFsRequest<Request = protocol::PathOpenReq>,
    {
        self.open_object(P::request(&received).0)
    }

    fn open_object(&self, open: protocol::PathOpen) -> ChoreoFsOpen<'static> {
        let Some(object) = self
            .choreofs
            .resolve(open.path().strip_prefix(b"/").unwrap_or(open.path()))
        else {
            return ChoreoFsOpen::denied(open, None, ERRNO_NOENT);
        };
        let Some(material) = self.materials.object(object) else {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_NOENT);
        };
        if open.fd_flags() & !1 != 0
            && !(open.fd_flags() == 4
                && matches!(material.kind, ChoreoFsObjectMaterialKind::Readdir(_)))
        {
            return ChoreoFsOpen::denied(open, Some(object), 58); // NOTSUP
        }
        if open.open_flags() & 5 == 5 {
            return ChoreoFsOpen::denied(open, Some(object), 20); // EXIST
        }
        if open.open_flags() & 2 != 0
            && !matches!(material.kind, ChoreoFsObjectMaterialKind::Readdir(_))
        {
            return ChoreoFsOpen::denied(open, Some(object), 54); // NOTDIR
        }
        if (open.open_flags() & 8 != 0 || open.fd_flags() & 1 != 0)
            && (open.rights_base() & FD_WRITE_RIGHT == 0
                || matches!(material.kind, ChoreoFsObjectMaterialKind::Readdir(_)))
        {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_ACCES);
        }
        let Some(required_rights) = material_required_rights(material, open.rights_base()) else {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_ACCES);
        };
        if !binding_allows_rights(material.binding(), required_rights) {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_ACCES);
        }
        let Some(fact) = self.ledger.object_with_rights(object, required_rights) else {
            return ChoreoFsOpen::denied(open, Some(object), ERRNO_ACCES);
        };
        let binding = material.binding();
        let selected = FdBinding {
            read: if required_rights & FD_READ_RIGHT != 0 {
                binding.read
            } else {
                None
            },
            write: if required_rights & FD_WRITE_RIGHT != 0 {
                binding.write
            } else {
                None
            },
            readdir: if required_rights & FD_READDIR_RIGHT != 0 {
                binding.readdir
            } else {
                None
            },
        };
        ChoreoFsOpen::opened(open, object, fact.fd(), selected)
    }

    /// Execute the received `FdReaddirReq` request against these object facts.
    pub fn fd_readdir<'r, P>(&self, received: ReceivedPayload<'r, P>) -> ChoreoFsReadDir<'r>
    where
        'a: 'r,
        P: ChoreoFsRequest<Request = protocol::FdReaddirReq>,
    {
        self.read_directory(P::request(&received).0)
    }

    fn read_directory(&self, read: protocol::FdReaddir) -> ChoreoFsReadDir<'a> {
        let Some(material) = self.material_for_fd(read.fd()) else {
            return ChoreoFsReadDir::denied(read, None, ERRNO_BADF);
        };
        let ChoreoFsObjectMaterialKind::Readdir(listing) = material.kind else {
            return ChoreoFsReadDir::denied(read, Some(material.object()), ERRNO_BADF);
        };
        ChoreoFsReadDir::ready(read, material.object(), listing)
    }

    /// Execute the received `FdReadReq` request against these object facts.
    pub fn fd_read<'r, P>(&self, received: ReceivedPayload<'r, P>) -> ChoreoFsRead<'r>
    where
        'a: 'r,
        P: ChoreoFsRequest<Request = protocol::FdReadReq>,
    {
        self.read_object(P::request(&received).0)
    }

    fn read_object(&self, read: protocol::FdRead) -> ChoreoFsRead<'a> {
        let Some(material) = self.material_for_fd(read.fd()) else {
            return ChoreoFsRead::denied(read, None, ERRNO_BADF);
        };
        let bytes = match material.kind {
            ChoreoFsObjectMaterialKind::Readable(bytes)
            | ChoreoFsObjectMaterialKind::ReadWrite(bytes)
                if material.binding.read.is_some()
                    && self
                        .ledger
                        .fd(read.fd())
                        .is_some_and(|fact| fact.rights & FD_READ_RIGHT != 0) =>
            {
                bytes
            }
            _ => return ChoreoFsRead::denied(read, Some(material.object()), ERRNO_BADF),
        };
        ChoreoFsRead::ready(read, material.object(), bytes)
    }

    /// Apply a received write whose WASI row matches the object binding.
    pub fn fd_write<'r, P>(&self, received: ReceivedPayload<'r, P>) -> ChoreoFsWrite<'r>
    where
        P: ChoreoFsRequest<Request = (protocol::FdWrite, protocol::FdWriteRow)>,
    {
        let (write, row) = P::request(&received);
        self.write_object(write, row)
    }

    fn write_object(
        &self,
        write: protocol::FdWrite,
        row: protocol::FdWriteRow,
    ) -> ChoreoFsWrite<'static> {
        match self
            .material_for_fd(write.fd())
            .map(|material| (material.object(), material.kind))
        {
            Some((
                object,
                ChoreoFsObjectMaterialKind::Writable | ChoreoFsObjectMaterialKind::ReadWrite(_),
            )) if self
                .ledger
                .fd(write.fd())
                .is_some_and(|fact| fact.rights & FD_WRITE_RIGHT != 0)
                && self
                    .material_for_fd(write.fd())
                    .is_some_and(|material| material.binding.write == Some(row)) =>
            {
                ChoreoFsWrite::ready(write, object)
            }
            Some((object, _)) => ChoreoFsWrite::denied(write, Some(object), ERRNO_ACCES),
            None => ChoreoFsWrite::denied(write, None, ERRNO_ACCES),
        }
    }

    /// Execute the received `FdFdstatGetReq` request against these object facts.
    pub fn fd_fdstat_get<P>(&self, received: ReceivedPayload<'_, P>) -> protocol::FdStatRet
    where
        P: ChoreoFsRequest<Request = protocol::FdFdstatGetReq>,
    {
        self.fd_stat(P::request(&received).0)
    }

    fn fd_stat(&self, request: protocol::FdRequest) -> protocol::FdStatRet {
        let fd = request.fd();
        match self.material_for_fd(fd) {
            Some(material) => {
                protocol::FdStatRet(FdStat::new(fd, rights_from_binding(material.binding())))
            }
            None => protocol::FdStatRet(FdStat::new_with_errno(fd, MemRights::Read, ERRNO_BADF)),
        }
    }

    /// Execute the received `FdFilestatGetReq` request against these object facts.
    pub fn fd_filestat_get<P>(&self, received: ReceivedPayload<'_, P>) -> protocol::FdFilestatRet
    where
        P: ChoreoFsRequest<Request = protocol::FdFilestatGetReq>,
    {
        self.file_stat(P::request(&received).0)
    }

    fn file_stat(&self, request: protocol::FdRequest) -> protocol::FdFilestatRet {
        protocol::FdFilestatRet(
            self.material_for_fd(request.fd())
                .map(Self::material_stat)
                .unwrap_or_else(|| protocol::FileStat::new_with_errno(0, 0, ERRNO_BADF)),
        )
    }

    /// Execute the received `PathFilestatGetReq` request against these object facts.
    pub fn path_filestat_get<P>(
        &self,
        received: ReceivedPayload<'_, P>,
    ) -> protocol::PathFilestatRet
    where
        P: ChoreoFsRequest<Request = protocol::PathFilestatGetReq>,
    {
        self.path_stat(P::request(&received).0)
    }

    fn path_stat(&self, request: protocol::PathFilestatGet) -> protocol::PathFilestatRet {
        protocol::PathFilestatRet(
            self.choreofs
                .resolve(request.path())
                .and_then(|object| self.materials.object(object))
                .map(Self::material_stat)
                .unwrap_or_else(|| protocol::FileStat::new_with_errno(0, 0, ERRNO_NOENT)),
        )
    }

    fn material_stat(material: ChoreoFsObjectMaterial<'_>) -> protocol::FileStat {
        match material.kind {
            ChoreoFsObjectMaterialKind::Readable(bytes)
            | ChoreoFsObjectMaterialKind::ReadWrite(bytes) => {
                protocol::FileStat::new(protocol::WASIP1_FILETYPE_REGULAR_FILE, bytes.len() as u64)
            }
            ChoreoFsObjectMaterialKind::Readdir(_) => {
                protocol::FileStat::new(protocol::WASIP1_FILETYPE_DIRECTORY, 0)
            }
            ChoreoFsObjectMaterialKind::Writable => {
                protocol::FileStat::new(protocol::WASIP1_FILETYPE_REGULAR_FILE, 0)
            }
            ChoreoFsObjectMaterialKind::Empty => {
                protocol::FileStat::new_with_errno(0, 0, ERRNO_NOENT)
            }
        }
    }

    /// Execute the received `FdCloseReq` request against these object facts.
    pub fn fd_close<P>(&self, received: ReceivedPayload<'_, P>) -> protocol::FdClosedRet
    where
        P: ChoreoFsRequest<Request = protocol::FdCloseReq>,
    {
        self.close_object(P::request(&received).0)
    }

    fn close_object(&self, request: protocol::FdRequest) -> protocol::FdClosedRet {
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
            .fd(fd)
            .and_then(|fact| self.materials.object(fact.object()))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChoreoFsOpen<'r> {
    borrow: core::marker::PhantomData<&'r mut ()>,
    request: protocol::PathOpen,
    object: Option<ObjectId>,
    fd: u8,
    binding: FdBinding,
    errno: u16,
}

impl<'r> ChoreoFsOpen<'r> {
    const fn opened(
        request: protocol::PathOpen,
        object: ObjectId,
        fd: u8,
        binding: FdBinding,
    ) -> Self {
        Self {
            borrow: core::marker::PhantomData,
            request,
            object: Some(object),
            fd,
            binding,
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::PathOpen, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            borrow: core::marker::PhantomData,
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

#[derive(Debug, PartialEq, Eq)]
pub struct ChoreoFsRead<'a> {
    borrow: core::marker::PhantomData<&'a mut ()>,
    request: protocol::FdRead,
    object: Option<ObjectId>,
    bytes: Option<&'a [u8]>,
    errno: u16,
}

impl<'a> ChoreoFsRead<'a> {
    const fn ready(request: protocol::FdRead, object: ObjectId, bytes: &'a [u8]) -> Self {
        Self {
            borrow: core::marker::PhantomData,
            request,
            object: Some(object),
            bytes: Some(bytes),
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::FdRead, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            borrow: core::marker::PhantomData,
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

#[derive(Debug, PartialEq, Eq)]
pub struct ChoreoFsReadDir<'a> {
    borrow: core::marker::PhantomData<&'a mut ()>,
    request: protocol::FdReaddir,
    object: Option<ObjectId>,
    listing: Option<&'a [u8]>,
    errno: u16,
}

impl<'a> ChoreoFsReadDir<'a> {
    const fn ready(request: protocol::FdReaddir, object: ObjectId, listing: &'a [u8]) -> Self {
        Self {
            borrow: core::marker::PhantomData,
            request,
            object: Some(object),
            listing: Some(listing),
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::FdReaddir, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            borrow: core::marker::PhantomData,
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

#[derive(Debug, PartialEq, Eq)]
pub struct ChoreoFsWrite<'r> {
    borrow: core::marker::PhantomData<&'r mut ()>,
    request: protocol::FdWrite,
    object: Option<ObjectId>,
    errno: u16,
}

impl<'r> ChoreoFsWrite<'r> {
    const fn ready(request: protocol::FdWrite, object: ObjectId) -> Self {
        Self {
            borrow: core::marker::PhantomData,
            request,
            object: Some(object),
            errno: ERRNO_SUCCESS,
        }
    }

    const fn denied(request: protocol::FdWrite, object: Option<ObjectId>, errno: u16) -> Self {
        Self {
            borrow: core::marker::PhantomData,
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
    let (required, applicable) = match material.kind {
        ChoreoFsObjectMaterialKind::ReadWrite(_) => {
            let requested = rights & (FD_READ_RIGHT | FD_WRITE_RIGHT);
            return (requested != 0).then_some(requested);
        }
        ChoreoFsObjectMaterialKind::Empty => return None,
        ChoreoFsObjectMaterialKind::Readable(_) => (FD_READ_RIGHT, FD_READ_RIGHT | FD_WRITE_RIGHT),
        ChoreoFsObjectMaterialKind::Readdir(_) => {
            (FD_READDIR_RIGHT, FD_READDIR_RIGHT | FD_WRITE_RIGHT)
        }
        ChoreoFsObjectMaterialKind::Writable => (FD_WRITE_RIGHT, FD_READ_RIGHT | FD_WRITE_RIGHT),
    };
    // WASI permits dropping only rights inapplicable to the object type:
    // readdir on a file, and byte reads on a directory. A conflicting file
    // read/write mode must still reject, rather than grant another capability.
    (rights & applicable == required).then_some(required)
}

const fn rights_from_binding(binding: FdBinding) -> MemRights {
    if binding.read.is_some() && binding.write.is_some() {
        MemRights::ReadWrite
    } else if binding.write.is_some() {
        MemRights::Write
    } else {
        MemRights::Read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static OBJECTS: ChoreoFsObjectSet<'static, 2> = ChoreoFsObjectSet::new([
        ChoreoFsObject::new(
            b"outputs/led/green",
            ObjectId(1),
            FdSpec::new(10, FD_WRITE_RIGHT, 100),
        ),
        ChoreoFsObject::new(b"outputs/led/red", ObjectId(2), FdSpec::new(11, 0b10, 101)),
    ]);

    static RUNTIME_OBJECTS: ChoreoFsObjectSet<'static, 3> = ChoreoFsObjectSet::new([
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
    fn path_open_rejects_absent_or_conflicting_io_rights_without_poisoning_reentry() {
        let choreofs = RUNTIME_OBJECTS.choreofs();
        for (path, expected) in [
            (&b"objects"[..], FD_READDIR_RIGHT),
            (&b"objects/log"[..], FD_READ_RIGHT),
            (&b"outputs/led/green"[..], FD_WRITE_RIGHT),
        ] {
            for bits in 0..8 {
                let requested = (if bits & 1 != 0 { FD_READ_RIGHT } else { 0 })
                    | (if bits & 2 != 0 { FD_WRITE_RIGHT } else { 0 })
                    | (if bits & 4 != 0 { FD_READDIR_RIGHT } else { 0 });
                for ancillary in [0, 1 << 21] {
                    let open = choreofs.open_object(
                        protocol::PathOpen::new(3, requested | ancillary, path).unwrap(),
                    );
                    let applicable = if expected == FD_READDIR_RIGHT {
                        FD_READDIR_RIGHT | FD_WRITE_RIGHT
                    } else {
                        FD_READ_RIGHT | FD_WRITE_RIGHT
                    };
                    let accepted = requested & applicable == expected;
                    assert_eq!(
                        open.is_open(),
                        accepted,
                        "path={path:?}, requested={requested:#x}"
                    );
                    if !accepted {
                        assert_eq!(open.fd(), None);
                        let response = open.opened_ret();
                        assert_eq!(response.0.errno(), ERRNO_ACCES);
                        assert_eq!(response.0.binding(), FdBinding::none());
                    }
                    // A rejected read must not consume the later valid write.
                    let valid =
                        choreofs.open_object(protocol::PathOpen::new(3, expected, path).unwrap());
                    assert!(valid.is_open());
                }
            }
        }
    }

    #[test]
    fn borrowed_read_write_files_keep_contents_and_attenuate_open_rights() {
        let contents = [7, 8, 9];
        let both = FdBinding {
            read: Some(protocol::FdReadRow::Base),
            write: Some(protocol::FdWriteRow::Object),
            readdir: None,
        };
        let objects = ChoreoFsObjectSet::new([ChoreoFsObject::read_write(
            b"file",
            ObjectId(40),
            FdSpec::new(6, FD_READ_RIGHT | FD_WRITE_RIGHT, 1),
            &contents,
            both,
        )]);
        let fs = objects.choreofs();
        for rights in [
            FD_READ_RIGHT,
            FD_WRITE_RIGHT,
            FD_READ_RIGHT | FD_WRITE_RIGHT,
        ] {
            let open = fs.open_object(protocol::PathOpen::new(3, rights, b"file").unwrap());
            assert!(open.is_open());
            assert_eq!(open.binding().read.is_some(), rights & FD_READ_RIGHT != 0);
            assert_eq!(open.binding().write.is_some(), rights & FD_WRITE_RIGHT != 0);
        }
        assert!(
            !fs.open_object(protocol::PathOpen::new(3, 0, b"file").unwrap())
                .is_open()
        );
        let (read, offset) = fs
            .read_object(protocol::FdRead::new(6, 2).unwrap())
            .read_from(1)
            .unwrap();
        assert_eq!(read.0.as_bytes(), &[8, 9]);
        assert_eq!(offset, 3);
        assert_eq!(fs.file_stat(protocol::FdRequest::new(6)).0.size(), 3);
        let read_only_truncate = protocol::PathOpen::new(3, FD_READ_RIGHT, b"file")
            .unwrap()
            .with_flags(8, 0)
            .unwrap();
        assert!(!fs.open_object(read_only_truncate).is_open());
        let write_truncate = protocol::PathOpen::new(3, FD_WRITE_RIGHT, b"file")
            .unwrap()
            .with_flags(8, 0)
            .unwrap();
        assert!(fs.open_object(write_truncate).is_open());
        assert_eq!(contents, [7, 8, 9]); // Admission itself never mutates file data.
        let exclusive = protocol::PathOpen::new(3, FD_WRITE_RIGHT, b"file")
            .unwrap()
            .with_flags(5, 0)
            .unwrap();
        assert_eq!(fs.open_object(exclusive).errno(), 20);
    }

    #[test]
    #[ignore = "exports actual ChoreoFS decisions for the Lean correspondence gate"]
    fn export_actual_path_rights_decisions() {
        use std::fmt::Write;
        static OBJECTS: ChoreoFsObjectSet<'static, 5> = ChoreoFsObjectSet::new([
            ChoreoFsObject::new(b"empty", ObjectId(20), FdSpec::new(7, 0, 0)),
            ChoreoFsObject::readable(
                b"reader",
                ObjectId(21),
                FdSpec::new(8, FD_READ_RIGHT, 0),
                b"",
                FdBinding::read(protocol::FdReadRow::Base),
            ),
            ChoreoFsObject::writable(
                b"writer",
                ObjectId(22),
                FdSpec::new(9, FD_WRITE_RIGHT, 0),
                FdBinding::write(protocol::FdWriteRow::Object),
            ),
            ChoreoFsObject::readdir(
                b"directory",
                ObjectId(23),
                FdSpec::new(10, FD_READDIR_RIGHT, 0),
                b"",
                FdBinding::readdir(protocol::FdReaddirRow::Base),
            ),
            ChoreoFsObject::read_write(
                b"file",
                ObjectId(24),
                FdSpec::new(11, FD_READ_RIGHT | FD_WRITE_RIGHT, 0),
                b"",
                FdBinding {
                    read: Some(protocol::FdReadRow::Base),
                    write: Some(protocol::FdWriteRow::Object),
                    readdir: None,
                },
            ),
        ]);
        let output = std::env::var("HIBANA_WASIP1_PATH_LEAN_EXPORT").expect("export destination");
        let choreofs = OBJECTS.choreofs();
        let mask = FD_READ_RIGHT | FD_WRITE_RIGHT | FD_READDIR_RIGHT;
        let mut lean = std::string::String::from("import PathRights\nnamespace WasiPathRights\n");
        let mut index = 0;
        for (kind, path) in [
            b"empty".as_slice(),
            b"reader",
            b"writer",
            b"directory",
            b"file",
        ]
        .into_iter()
        .enumerate()
        {
            for bits in 0..8 {
                let io = (if bits & 1 != 0 { FD_READ_RIGHT } else { 0 })
                    | (if bits & 2 != 0 { FD_WRITE_RIGHT } else { 0 })
                    | (if bits & 4 != 0 { FD_READDIR_RIGHT } else { 0 });
                for ancillary in [0, 1 << 21, 1 << 63, !mask] {
                    let requested = io | ancillary;
                    let open =
                        choreofs.open_object(protocol::PathOpen::new(3, requested, path).unwrap());
                    let accepted = open.is_open();
                    if !accepted {
                        assert_eq!(open.fd(), None);
                        assert_eq!(open.opened_ret().0.binding(), FdBinding::none());
                    }
                    writeln!(lean, "theorem actual_path_decision_{index} : admitted {kind} {requested} = {accepted} := by decide").unwrap();
                    writeln!(lean, "#print axioms actual_path_decision_{index}").unwrap();
                    index += 1;
                }
            }
        }
        assert_eq!(index, 160);
        lean.push_str("end WasiPathRights\n");
        std::fs::write(output, lean).unwrap();
        std::println!("Exported {index} actual ChoreoFS path admission decisions");
    }

    #[test]
    fn path_open_still_requires_both_actual_binding_and_ledger_right() {
        static OBJECTS: ChoreoFsObjectSet<'static, 4> = ChoreoFsObjectSet::new([
            ChoreoFsObject::writable(
                b"no-binding",
                ObjectId(30),
                FdSpec::new(7, FD_WRITE_RIGHT, 0),
                FdBinding::none(),
            ),
            ChoreoFsObject::writable(
                b"no-ledger-right",
                ObjectId(31),
                FdSpec::new(8, FD_READ_RIGHT, 0),
                FdBinding::write(protocol::FdWriteRow::Object),
            ),
            ChoreoFsObject::readable(
                b"wrong-binding",
                ObjectId(32),
                FdSpec::new(9, FD_READ_RIGHT, 0),
                b"",
                FdBinding::write(protocol::FdWriteRow::Object),
            ),
            ChoreoFsObject::writable(
                b"valid",
                ObjectId(33),
                FdSpec::new(10, FD_WRITE_RIGHT, 0),
                FdBinding::write(protocol::FdWriteRow::Object),
            ),
        ]);
        let choreofs = OBJECTS.choreofs();
        for (path, right) in [
            (b"no-binding".as_slice(), FD_WRITE_RIGHT),
            (b"no-ledger-right", FD_WRITE_RIGHT),
            (b"wrong-binding", FD_READ_RIGHT),
        ] {
            let denied = choreofs.open_object(protocol::PathOpen::new(3, right, path).unwrap());
            assert_eq!(denied.fd(), None);
            assert_eq!(denied.opened_ret().0.errno(), ERRNO_ACCES);
        }
        assert!(
            choreofs
                .open_object(protocol::PathOpen::new(3, FD_WRITE_RIGHT, b"valid").unwrap())
                .is_open()
        );
    }

    #[test]
    fn choreofs_builds_object_operation_tokens() {
        let choreofs = RUNTIME_OBJECTS.choreofs();

        let open = choreofs.open_object(
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

        let mixed_dir_open = choreofs.open_object(
            protocol::PathOpen::new(3, FD_READ_RIGHT | FD_READDIR_RIGHT, b"objects")
                .expect("path_open"),
        );
        assert_eq!(mixed_dir_open.fd(), Some(4));

        let mixed_read_open = choreofs.open_object(
            protocol::PathOpen::new(3, FD_READ_RIGHT | FD_READDIR_RIGHT, b"objects/log")
                .expect("path_open"),
        );
        assert_eq!(mixed_read_open.fd(), Some(5));

        let readdir = choreofs
            .read_directory(protocol::FdReaddir::new(4, 0, 16).expect("fd_readdir request"));
        assert_eq!(readdir.object(), Some(ObjectId(10)));
        let listing = readdir.read_dir().expect("fd_readdir ret");
        assert_eq!(listing.0.as_bytes(), b"log\n");

        let read = protocol::FdRead::new(5, 30).expect("fd_read request");
        let read = choreofs.read_object(read);
        assert_eq!(read.object(), Some(ObjectId(11)));
        let (chunk, next_offset) = read.read_from(0).expect("fd_read ret");
        assert_eq!(chunk.0.as_bytes(), b"session=attached\n");
        assert_eq!(next_offset, b"session=attached\n".len());

        let denied_read =
            choreofs.read_object(protocol::FdRead::new(9, 30).expect("fd_read request"));
        assert_eq!(denied_read.object(), None);
        assert_eq!(denied_read.errno(), ERRNO_BADF);
        let (denied_read, _) = denied_read.read_from(0).expect("fd_read ret");
        assert_eq!(denied_read.0.errno(), ERRNO_BADF);

        let denied_stat = choreofs.fd_stat(protocol::FdRequest::new(9));
        assert_eq!(denied_stat.0.fd(), 9);
        assert_eq!(denied_stat.0.errno(), ERRNO_BADF);

        let denied_close = choreofs.close_object(protocol::FdRequest::new(9));
        assert_eq!(denied_close.0.fd(), 9);
        assert_eq!(denied_close.0.errno(), ERRNO_BADF);

        let write = choreofs.write_object(
            protocol::FdWrite::new(6, b"1").expect("fd_write request"),
            protocol::FdWriteRow::Object,
        );
        assert_eq!(write.object(), Some(ObjectId(12)));
        assert_eq!(write.bytes(), b"1");
        let written = write.written();
        assert_eq!(written.0.written(), 1);
        assert_eq!(written.0.errno(), ERRNO_SUCCESS);

        let denied = choreofs.open_object(
            protocol::PathOpen::new(3, FD_WRITE_RIGHT, b"objects/log").expect("path_open"),
        );
        assert_eq!(denied.object(), Some(ObjectId(11)));
        assert!(!denied.is_open());
        let denied = denied.opened_ret();
        assert_eq!(denied.0.errno(), ERRNO_ACCES);
    }
}

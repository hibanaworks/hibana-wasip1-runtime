use super::wasi_shell_demo_lib::transport;

use hibana::{
    g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::project,
        transport::EndpointBinding,
        wire::{CodecError, Payload, ReceivedPayload, WireEncode, WirePayload},
    },
};
use hibana_wasip1_runtime::{
    choreofs::{ChoreoFsObject, ChoreoFsObjectSet, ChoreoFsRequest, FdSpec, ObjectId},
    protocol::*,
};
use std::rc::Rc;

struct CloseWithTag(u8, u8);
impl WireEncode for CloseWithTag {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        [self.0, self.1].encode_into(out)
    }
}
impl WirePayload for CloseWithTag {
    const SCHEMA_ID: u32 = 0x4430_0001;
    type Decoded<'a> = ReceivedPayload<'a, Self>;
    fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
        <[u8; 2]>::validate_payload(input)
    }
    fn decode_validated_payload<'a>(input: ReceivedPayload<'a, Self>) -> Self::Decoded<'a> {
        input
    }
}
impl ChoreoFsRequest for CloseWithTag {
    type Request = FdCloseReq;
    fn request(input: &ReceivedPayload<'_, Self>) -> FdCloseReq {
        FdCloseReq(FdRequest::new(input.as_bytes()[0]))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn parallel_received_requests_and_custom_payloads_materialize_objects() {
    let objects = ChoreoFsObjectSet::new([
        ChoreoFsObject::writable(
            b"output",
            ObjectId(1),
            FdSpec::new(4, 1 << 6, 1),
            FdBinding::write(FdWriteRow::Object),
        ),
        ChoreoFsObject::readable(
            b"input",
            ObjectId(2),
            FdSpec::new(5, 1 << 1, 1),
            b"hello",
            FdBinding::read(FdReadRow::Base),
        ),
    ]);
    let fs = objects.choreofs();
    type Close = g::Msg<201, CloseWithTag>;
    type Closed = g::Msg<202, FdClosedRet>;
    let global = g::seq(
        g::par(
            g::seq(
                g::send::<0, 1, PathOpenReqMsg>(),
                g::seq(
                    g::send::<1, 0, PathOpenRetMsg>(),
                    g::seq(
                        g::send::<0, 1, FdWriteObjectReqMsg>(),
                        g::send::<1, 0, FdWriteObjectRetMsg>(),
                    ),
                ),
            ),
            g::seq(
                g::send::<2, 1, FdReadReqMsg>(),
                g::send::<1, 2, FdReadRetMsg>(),
            ),
        ),
        g::seq(g::send::<0, 1, Close>(), g::send::<1, 0, Closed>()),
    );
    let programs = (
        project::<0>(&global),
        project::<1>(&global),
        project::<2>(&global),
    );
    let sid = SessionId::new(1);
    // SAFETY: this test alone owns the transport's physical resource boundary.
    let binding = Rc::new(unsafe { EndpointBinding::new() });
    let transport = transport::InProcessTransport::new(sid, 9, binding);
    let mut slab = [0; 32768];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage
        .init()
        .rendezvous(&mut slab, transport.clone())
        .unwrap();
    let mut writer = rv.enter(sid, &programs.0).unwrap();
    let mut service = rv.enter(sid, &programs.1).unwrap();
    let mut reader = rv.enter(sid, &programs.2).unwrap();
    let ((), (), ()) = tokio::try_join!(
        async {
            writer
                .send::<PathOpenReqMsg>(&PathOpenReq(PathOpen::new(3, 1 << 6, b"/output").unwrap()))
                .await?;
            assert_eq!(writer.recv::<PathOpenRetMsg>().await?.0.fd(), 4);
            writer
                .send::<FdWriteObjectReqMsg>(&FdWriteObjectReq(FdWrite::new(4, b"new").unwrap()))
                .await?;
            assert_eq!(writer.recv::<FdWriteObjectRetMsg>().await?.0.written(), 3);
            writer.send::<Close>(&CloseWithTag(4, 7)).await?;
            assert_eq!(writer.recv::<Closed>().await?.0.errno(), 0);
            Ok::<_, hibana::EndpointError>(())
        },
        async {
            reader
                .send::<FdReadReqMsg>(&FdReadReq(FdRead::new(5, 5).unwrap()))
                .await?;
            assert_eq!(reader.recv::<FdReadRetMsg>().await?.0.as_bytes(), b"hello");
            Ok(())
        },
        async {
            {
                let [mut writes, mut reads] = service.par()?;
                tokio::try_join!(
                    async {
                        let request = writes.recv::<PathOpenReqMsg>().await?;
                        let opened = fs.path_open(request).opened_ret();
                        writes.send::<PathOpenRetMsg>(&opened).await?;
                        let request = writes.recv::<FdWriteObjectReqMsg>().await?;
                        let operation = fs.fd_write(request);
                        assert_eq!(operation.bytes(), b"new");
                        let written = operation.written();
                        writes.send::<FdWriteObjectRetMsg>(&written).await?;
                        Ok::<_, hibana::EndpointError>(())
                    },
                    async {
                        let request = reads.recv::<FdReadReqMsg>().await?;
                        let (data, _) = fs.fd_read(request).read_from(0).unwrap();
                        reads.send::<FdReadRetMsg>(&data).await
                    },
                )?;
            }
            let request = service.recv::<Close>().await?;
            assert_eq!(request.as_bytes()[1], 7);
            let closed = fs.fd_close(request);
            service.send::<Closed>(&closed).await
        }
    )
    .unwrap();
    assert!(transport.is_drained());
}

#[tokio::test(flavor = "current_thread")]
async fn fd_metadata_receive_cannot_be_decoded_as_close() {
    let global = g::send::<0, 1, FdFdstatGetReqMsg>();
    let programs = (project::<0>(&global), project::<1>(&global));
    let sid = SessionId::new(2);
    // SAFETY: this test alone owns the transport's physical resource boundary.
    let binding = Rc::new(unsafe { EndpointBinding::new() });
    let transport = transport::InProcessTransport::new(sid, 9, binding);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, transport).unwrap();
    let mut sender = rv.enter(sid, &programs.0).unwrap();
    let mut receiver = rv.enter(sid, &programs.1).unwrap();
    sender
        .send::<FdFdstatGetReqMsg>(&FdFdstatGetReq(FdRequest::new(4)))
        .await
        .unwrap();
    type WrongClose = g::Msg<LABEL_WASI_FD_FDSTAT_GET, FdCloseReq>;
    let error = receiver.recv::<WrongClose>().await.unwrap_err();
    assert!(
        format!("{error:?}").contains("PayloadMismatch"),
        "{error:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn received_write_rows_must_match_the_object_binding() {
    for binding_row in [FdWriteRow::Base, FdWriteRow::Object] {
        for sent_row in [FdWriteRow::Base, FdWriteRow::Object] {
            let objects = ChoreoFsObjectSet::new([ChoreoFsObject::writable(
                b"output",
                ObjectId(1),
                FdSpec::new(4, 1 << 6, 1),
                FdBinding::write(binding_row),
            )]);
            let fs = objects.choreofs();
            let global = g::route(
                g::send::<0, 1, FdWriteReqMsg>(),
                g::send::<0, 1, FdWriteObjectReqMsg>(),
            );
            let programs = (project::<0>(&global), project::<1>(&global));
            let sid = SessionId::new(3);
            // SAFETY: this test alone owns the transport's physical resource boundary.
            let binding = Rc::new(unsafe { EndpointBinding::new() });
            let transport = transport::InProcessTransport::new(sid, 9, binding);
            let mut slab = [0; 8192];
            let mut storage = SessionKitStorage::uninit();
            let rv = storage
                .init()
                .rendezvous(&mut slab, transport.clone())
                .unwrap();
            let mut sender = rv.enter(sid, &programs.0).unwrap();
            let mut receiver = rv.enter(sid, &programs.1).unwrap();
            match sent_row {
                FdWriteRow::Base => sender
                    .send::<FdWriteReqMsg>(&FdWriteReq(FdWrite::new(4, b"new").unwrap()))
                    .await
                    .unwrap(),
                FdWriteRow::Object => sender
                    .send::<FdWriteObjectReqMsg>(&FdWriteObjectReq(
                        FdWrite::new(4, b"new").unwrap(),
                    ))
                    .await
                    .unwrap(),
            }
            let branch = receiver.offer().await.unwrap();
            let operation = match branch.label() {
                LABEL_WASI_FD_WRITE => fs.fd_write(branch.recv::<FdWriteReqMsg>().await.unwrap()),
                LABEL_WASI_FD_WRITE_OBJECT => {
                    fs.fd_write(branch.recv::<FdWriteObjectReqMsg>().await.unwrap())
                }
                label => panic!("unexpected write label {label}"),
            };
            assert_eq!(operation.is_writable(), sent_row == binding_row);
            let response = operation.written();
            assert_eq!(
                response.0.errno(),
                if sent_row == binding_row { 0 } else { 2 }
            );
            assert!(transport.is_drained());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn base_write_receive_cannot_be_decoded_as_object_write() {
    let global = g::send::<0, 1, FdWriteReqMsg>();
    let programs = (project::<0>(&global), project::<1>(&global));
    let sid = SessionId::new(4);
    // SAFETY: this test alone owns the transport's physical resource boundary.
    let binding = Rc::new(unsafe { EndpointBinding::new() });
    let transport = transport::InProcessTransport::new(sid, 9, binding);
    let mut slab = [0; 8192];
    let mut storage = SessionKitStorage::uninit();
    let rv = storage.init().rendezvous(&mut slab, transport).unwrap();
    let mut sender = rv.enter(sid, &programs.0).unwrap();
    let mut receiver = rv.enter(sid, &programs.1).unwrap();
    sender
        .send::<FdWriteReqMsg>(&FdWriteReq(FdWrite::new(4, b"new").unwrap()))
        .await
        .unwrap();
    type WrongObjectWrite = g::Msg<LABEL_WASI_FD_WRITE, FdWriteObjectReq>;
    let error = receiver.recv::<WrongObjectWrite>().await.unwrap_err();
    assert!(
        format!("{error:?}").contains("PayloadMismatch"),
        "{error:?}"
    );
}

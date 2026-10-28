use core::{
    cell::RefCell,
    task::{Context, Poll, Waker},
};
use std::{collections::VecDeque, rc::Rc};

use hibana::runtime::{
    ids::SessionId,
    transport::{
        EndpointBinding, FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError,
    },
    wire::Payload,
};

#[derive(Clone)]
pub struct InProcessTransport {
    state: Rc<RefCell<TransportState>>,
    session: SessionId,
    resource: u8,
    binding: Rc<EndpointBinding>,
}

impl InProcessTransport {
    pub fn new(session: SessionId, resource: u8, binding: Rc<EndpointBinding>) -> Self {
        Self {
            state: Rc::new(RefCell::new(TransportState::new())),
            session,
            resource,
            binding,
        }
    }

    pub fn is_drained(&self) -> bool {
        self.state.borrow().frames.is_empty()
    }
}

struct TransportState {
    frames: VecDeque<Frame>,
    waiters: Vec<RecvWaiter>,
}

impl TransportState {
    fn new() -> Self {
        Self {
            frames: VecDeque::new(),
            waiters: Vec::new(),
        }
    }
}

struct Frame {
    session_id: SessionId,
    lane: u8,
    source_role: u8,
    target_role: u8,
    label: u16,
    payload: Vec<u8>,
}

struct RecvWaiter {
    session_id: SessionId,
    local_role: u8,
    lane: u8,
    waker: Waker,
}

pub struct Tx {
    session_id: SessionId,
    local_role: u8,
}

pub struct Rx {
    session_id: SessionId,
    local_role: u8,
    lane: u8,
    current: Option<Frame>,
}

impl Transport for InProcessTransport {
    fn initial_endpoint_binding(&self, session: SessionId, role: u8) -> Option<&EndpointBinding> {
        (session == self.session && role == self.resource).then_some(self.binding.as_ref())
    }

    type Tx<'a>
        = Tx
    where
        Self: 'a;
    type Rx<'a>
        = Rx
    where
        Self: 'a;

    fn open<'a>(&'a self, port: PortOpen) -> (Self::Tx<'a>, Self::Rx<'a>) {
        (
            Tx {
                session_id: port.session_id(),
                local_role: port.local_role(),
            },
            Rx {
                session_id: port.session_id(),
                local_role: port.local_role(),
                lane: port.lane(),
                current: None,
            },
        )
    }

    fn poll_send<'f>(
        &self,
        tx: &mut Self::Tx<'_>,
        outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>> {
        let target_role = outgoing.target_role();
        let lane = outgoing.lane();
        let mut state = self.state.borrow_mut();
        state.frames.push_back(Frame {
            session_id: tx.session_id,
            lane,
            source_role: tx.local_role,
            target_role,
            label: outgoing.frame_label().raw(),
            payload: outgoing.payload().as_bytes().to_vec(),
        });
        state.wake_recv_waiters(tx.session_id, target_role, lane);
        Poll::Ready(Ok(()))
    }

    fn cancel_send(&self, _tx: &mut Self::Tx<'_>) {}

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'_>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if rx.current.is_some() {
            rx.current = None;
        }
        if rx.current.is_none() {
            let frame = {
                let mut state = self.state.borrow_mut();
                let index = state.frames.iter().position(|frame| {
                    frame.session_id == rx.session_id
                        && frame.target_role == rx.local_role
                        && frame.lane == rx.lane
                });
                index.and_then(|idx| state.frames.remove(idx))
            };
            let Some(frame) = frame else {
                self.state.borrow_mut().store_recv_waiter(
                    rx.session_id,
                    rx.local_role,
                    rx.lane,
                    _cx.waker(),
                );
                return Poll::Pending;
            };
            rx.current = Some(frame);
        }

        let frame = rx.current.as_ref().expect("current frame");
        let header = frame_header(
            frame.session_id,
            frame.lane,
            frame.source_role,
            frame.target_role,
            frame.label,
        );
        Poll::Ready(Ok(ReceivedFrame::framed(
            header,
            Payload::new(frame.payload.as_slice()),
        )))
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        if let Some(frame) = rx.current.take() {
            self.state.borrow_mut().frames.push_front(frame);
        }
        Ok(())
    }
}

impl TransportState {
    fn store_recv_waiter(
        &mut self,
        session_id: SessionId,
        local_role: u8,
        lane: u8,
        waker: &Waker,
    ) {
        if let Some(waiter) = self.waiters.iter_mut().find(|waiter| {
            waiter.session_id == session_id
                && waiter.local_role == local_role
                && waiter.lane == lane
        }) {
            waiter.waker = waker.clone();
            return;
        }
        self.waiters.push(RecvWaiter {
            session_id,
            local_role,
            lane,
            waker: waker.clone(),
        });
    }

    fn wake_recv_waiters(&mut self, session_id: SessionId, local_role: u8, lane: u8) {
        let mut index = 0usize;
        while index < self.waiters.len() {
            if self.waiters[index].session_id == session_id
                && self.waiters[index].local_role == local_role
                && self.waiters[index].lane == lane
            {
                let waiter = self.waiters.remove(index);
                waiter.waker.wake();
            } else {
                index += 1;
            }
        }
    }
}

fn frame_header(
    session: SessionId,
    lane: u8,
    source_role: u8,
    target_role: u8,
    label: u16,
) -> FrameHeader {
    let session = session.raw().to_be_bytes();
    let label = label.to_be_bytes();
    FrameHeader::from_bytes([
        session[0],
        session[1],
        session[2],
        session[3],
        lane,
        source_role,
        target_role,
        label[0],
        label[1],
    ])
}

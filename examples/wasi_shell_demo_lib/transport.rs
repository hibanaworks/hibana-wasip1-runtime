use core::{
    cell::RefCell,
    task::{Context, Poll, Waker},
};
use std::{
    collections::VecDeque,
    rc::Rc,
    thread,
    time::{Duration, Instant},
};

use hibana::runtime::{
    ids::SessionId,
    transport::{FrameHeader, Outgoing, PortOpen, ReceivedFrame, Transport, TransportError},
    wire::Payload,
};

#[derive(Clone)]
pub struct InProcessTransport {
    state: Rc<RefCell<TransportState>>,
}

impl InProcessTransport {
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(TransportState::new())),
        }
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
    label: u8,
    payload: Vec<u8>,
}

struct RecvWaiter {
    local_role: u8,
    lane: u8,
    waker: Waker,
}

pub struct Tx {
    session_id: SessionId,
    local_role: u8,
}

pub struct Rx {
    local_role: u8,
    lane: u8,
    current: Option<Frame>,
    pending_since: Option<Instant>,
    deadline_armed: bool,
}

const RECV_STALL_TIMEOUT_MS: u64 = 100;

impl Transport for InProcessTransport {
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
                local_role: port.local_role(),
                lane: port.lane(),
                current: None,
                pending_since: None,
                deadline_armed: false,
            },
        )
    }

    fn poll_send<'a, 'f>(
        &self,
        tx: &'a mut Self::Tx<'a>,
        outgoing: Outgoing<'f>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), TransportError>>
    where
        'a: 'f,
    {
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
        state.wake_recv_waiters(target_role, lane);
        Poll::Ready(Ok(()))
    }

    fn cancel_send<'a>(&self, _tx: &'a mut Self::Tx<'a>) {}

    fn poll_recv<'a>(
        &'a self,
        rx: &'a mut Self::Rx<'a>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<ReceivedFrame<'a>, TransportError>> {
        if rx.current.is_some() {
            rx.current = None;
        }
        if rx.current.is_none() {
            let frame = {
                let mut state = self.state.borrow_mut();
                let index = state
                    .frames
                    .iter()
                    .position(|frame| frame.target_role == rx.local_role && frame.lane == rx.lane);
                index.and_then(|idx| state.frames.remove(idx))
            };
            let Some(frame) = frame else {
                let since = rx.pending_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(RECV_STALL_TIMEOUT_MS) {
                    return Poll::Ready(Err(TransportError::Deadline));
                }
                self.state
                    .borrow_mut()
                    .store_recv_waiter(rx.local_role, rx.lane, _cx.waker());
                if !rx.deadline_armed {
                    rx.deadline_armed = true;
                    let waker = _cx.waker().clone();
                    let _watchdog = thread::spawn(move || {
                        thread::sleep(Duration::from_millis(RECV_STALL_TIMEOUT_MS));
                        waker.wake();
                    });
                }
                return Poll::Pending;
            };
            rx.pending_since = None;
            rx.deadline_armed = false;
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
        let bytes: &'a [u8] = unsafe { &*(frame.payload.as_slice() as *const [u8]) };
        Poll::Ready(Ok(ReceivedFrame::framed(header, Payload::new(bytes))))
    }

    fn requeue<'a>(&self, rx: &mut Self::Rx<'a>) -> Result<(), TransportError> {
        if let Some(frame) = rx.current.take() {
            self.state.borrow_mut().frames.push_front(frame);
        }
        Ok(())
    }
}

impl TransportState {
    fn store_recv_waiter(&mut self, local_role: u8, lane: u8, waker: &Waker) {
        if let Some(waiter) = self
            .waiters
            .iter_mut()
            .find(|waiter| waiter.local_role == local_role && waiter.lane == lane)
        {
            waiter.waker = waker.clone();
            return;
        }
        self.waiters.push(RecvWaiter {
            local_role,
            lane,
            waker: waker.clone(),
        });
    }

    fn wake_recv_waiters(&mut self, local_role: u8, lane: u8) {
        let mut index = 0usize;
        while index < self.waiters.len() {
            if self.waiters[index].local_role == local_role && self.waiters[index].lane == lane {
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
    label: u8,
) -> FrameHeader {
    let session = session.raw().to_be_bytes();
    FrameHeader::from_bytes([
        session[0],
        session[1],
        session[2],
        session[3],
        lane,
        source_role,
        target_role,
        label,
    ])
}

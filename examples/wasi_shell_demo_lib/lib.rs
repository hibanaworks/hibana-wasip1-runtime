pub mod error;
pub mod transport;

/// Poll the closed in-process examples without an operating-system reactor.
/// Every pending operation is driven by another local participant. Continuous
/// polling also makes these examples runnable under Miri.
pub struct Events;

impl hibana_runtime::Driver for Events {
    fn waker(&self) -> core::task::Waker {
        core::task::Waker::noop().clone()
    }

    fn wait(&mut self) {
        std::thread::yield_now();
    }
}

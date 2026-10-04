use super::{Interpreter, PollOneoffCall, WasmError, diagnostic_message_code};
use crate::protocol::{PollOneoff, PollReady};

pub(super) fn request<'a>(
    core: &'a Interpreter<'_>,
    call: PollOneoffCall,
) -> Result<PollOneoff<'a>, WasmError> {
    let count = call.nsubscriptions as usize;
    if count == 0 || count > crate::exchange::FD_BINDING_CAPACITY {
        return Err(unsupported!(
            "poll subscription count exceeds the fd domain"
        ));
    }
    core.require_memory_range(call.in_ptr, count * 48)?;
    core.require_memory_range(call.out_ptr, count * 32)?;
    core.require_memory_range(call.nevents, 4)?;
    let output = call.out_ptr as usize;
    let count_at = call.nevents as usize;
    if output < count_at + 4 && count_at < output + count * 32 {
        return Err(invalid!("poll event and count outputs overlap"));
    }
    let start = call.in_ptr as usize;
    let (records, _) = core.memory.as_slice()[start..start + count * 48].as_chunks::<48>();
    PollOneoff::new(records).map_err(|_| invalid!("invalid poll subscription"))
}

/// All input records, event identities and output ranges are checked before
/// this exclusive pair of output borrows can be constructed. Consuming it
/// performs no fallible operation and cannot modify subscription validation.
pub(super) struct PreparedWriteback<'memory, 'events> {
    events: &'memory mut [u8],
    count: &'memory mut [u8],
    ready: PollReady<'events>,
}

impl<'memory, 'events> PreparedWriteback<'memory, 'events> {
    pub(super) fn new(
        core: &'memory mut Interpreter<'_>,
        call: PollOneoffCall,
        ready: &'events [[u8; 32]],
    ) -> Result<Self, WasmError> {
        let requested = request(core, call)?;
        let validated = PollReady::new(ready).map_err(|_| invalid!("invalid poll events"))?;
        if ready.len() > requested.subscriptions().len() {
            return Err(invalid!("poll completion exceeds subscriptions"));
        }
        // Completions retain subscription order and consume each matching
        // occurrence once; duplicated userdata never permits an extra event.
        let mut remaining = requested.subscriptions();
        for event in ready {
            let Some(index) = remaining.iter().position(|subscription| {
                subscription[..8] == event[..8] && subscription[8] == event[10]
            }) else {
                return Err(invalid!(
                    "poll event does not match a subscription occurrence"
                ));
            };
            remaining = &remaining[index + 1..];
        }
        let event_start = call.out_ptr as usize;
        let count_start = call.nevents as usize;
        let event_len = ready.len() * 32;
        let memory = core.memory.as_mut_slice();
        let (events, count) = if event_start < count_start {
            let (before, after) = memory.split_at_mut(count_start);
            (
                &mut before[event_start..event_start + event_len],
                &mut after[..4],
            )
        } else {
            let (before, after) = memory.split_at_mut(event_start);
            (
                &mut after[..event_len],
                &mut before[count_start..count_start + 4],
            )
        };
        Ok(Self {
            events,
            count,
            ready: validated,
        })
    }

    pub(super) fn commit(self) {
        self.ready.write_events(self.events);
        self.count
            .copy_from_slice(&(self.ready.events().len() as u32).to_le_bytes());
    }
}

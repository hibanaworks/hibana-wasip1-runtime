use super::{CodecError, PollOneoffReq, PollReadyRet, schema};
use hibana::runtime::wire::{Payload, WireEncode, WirePayload};

const CAPACITY: usize = crate::exchange::FD_BINDING_CAPACITY;

/// Borrowed, validated WASI Preview 1 subscription records (48 bytes each).
/// Clock precision, relative/absolute flags and fd interests retain their ABI values.
#[derive(Clone, Copy, Debug)]
pub struct PollOneoff<'a> {
    records: &'a [[u8; 48]],
}

impl PartialEq for PollOneoff<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.records.len() == other.records.len()
            && self.records.iter().zip(other.records).all(|(left, right)| {
                subscription_fields(left)
                    .iter()
                    .all(|&(start, end)| left[start..end] == right[start..end])
            })
    }
}
impl Eq for PollOneoff<'_> {}

impl<'a> PollOneoff<'a> {
    pub fn new(subscriptions: &'a [[u8; 48]]) -> Result<Self, CodecError> {
        check_count(subscriptions.len())?;
        for input in subscriptions {
            match input[8] {
                0 => {
                    let clock = u32::from_le_bytes(input[16..20].try_into().expect("fixed record"));
                    let flags = u16::from_le_bytes(input[40..42].try_into().expect("fixed record"));
                    if clock > 3 || flags > 1 {
                        return Err(CodecError::Malformed);
                    }
                }
                1 | 2 => {
                    let fd = u32::from_le_bytes(input[16..20].try_into().expect("fixed record"));
                    if fd > u8::MAX as u32 {
                        return Err(CodecError::Malformed);
                    }
                }
                _ => return Err(CodecError::Malformed),
            }
        }
        Ok(Self {
            records: subscriptions,
        })
    }

    pub const fn subscriptions(&self) -> &'a [[u8; 48]] {
        self.records
    }
}

/// Borrowed, validated WASI events. Readiness never grants execution authority.
#[derive(Clone, Copy, Debug)]
pub struct PollReady<'a> {
    records: &'a [[u8; 32]],
}

impl PartialEq for PollReady<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.records.len() == other.records.len()
            && self.records.iter().zip(other.records).all(|(left, right)| {
                event_fields(left)
                    .iter()
                    .all(|&(start, end)| left[start..end] == right[start..end])
            })
    }
}
impl Eq for PollReady<'_> {}

impl<'a> PollReady<'a> {
    pub fn new(events: &'a [[u8; 32]]) -> Result<Self, CodecError> {
        check_count(events.len())?;
        for input in events {
            match input[10] {
                0 => (),
                1 | 2 => {
                    let flags = u16::from_le_bytes(input[24..26].try_into().expect("fixed record"));
                    if flags > 1 {
                        return Err(CodecError::Malformed);
                    }
                }
                _ => return Err(CodecError::Malformed),
            }
        }
        Ok(Self { records: events })
    }

    pub const fn events(&self) -> &'a [[u8; 32]] {
        self.records
    }

    pub(crate) fn write_events(self, out: &mut [u8]) {
        copy_records(out, self.records, event_fields);
    }
}

fn check_count(count: usize) -> Result<(), CodecError> {
    if count == 0 || count > CAPACITY {
        Err(CodecError::Malformed)
    } else {
        Ok(())
    }
}

// Each ABI union selects one set of meaningful ranges. Encoding, validation
// and equality share those ranges; guest padding is never part of a value.
fn subscription_fields(input: &[u8; 48]) -> [(usize, usize); 3] {
    [(0, 9), (16, 20), (24, if input[8] == 0 { 42 } else { 24 })]
}

fn event_fields(input: &[u8; 32]) -> [(usize, usize); 3] {
    if input[10] != 0 && input[8..10] == [0, 0] {
        [(0, 11), (16, 24), (24, 26)]
    } else {
        [(0, 11), (16, 16), (24, 24)]
    }
}

fn copy_records<const N: usize>(
    out: &mut [u8],
    records: &[[u8; N]],
    fields: fn(&[u8; N]) -> [(usize, usize); 3],
) {
    for (output, input) in out.chunks_exact_mut(N).zip(records) {
        output.fill(0);
        for (start, end) in fields(input) {
            output[start..end].copy_from_slice(&input[start..end]);
        }
    }
}

fn is_canonical<const N: usize>(
    record: &[u8; N],
    fields: fn(&[u8; N]) -> [(usize, usize); 3],
) -> bool {
    let mut end = 0;
    for (start, next) in fields(record) {
        if record[end..start].iter().any(|byte| *byte != 0) {
            return false;
        }
        end = next;
    }
    record[end..].iter().all(|byte| *byte == 0)
}

fn encode<const N: usize>(
    out: &mut [u8],
    records: &[[u8; N]],
    fields: fn(&[u8; N]) -> [(usize, usize); 3],
) -> Result<usize, CodecError> {
    let len = 1 + records.len() * N;
    if out.len() < len {
        return Err(CodecError::Truncated);
    }
    out[0] = records.len() as u8;
    copy_records(&mut out[1..len], records, fields);
    Ok(len)
}

fn decode<const N: usize>(
    bytes: &[u8],
    fields: fn(&[u8; N]) -> [(usize, usize); 3],
) -> Result<&[[u8; N]], CodecError> {
    let Some((&count, data)) = bytes.split_first() else {
        return Err(CodecError::Truncated);
    };
    check_count(count as usize)?;
    let (records, suffix) = data.as_chunks::<N>();
    if !suffix.is_empty()
        || records.len() != count as usize
        || records.iter().any(|record| !is_canonical(record, fields))
    {
        return Err(CodecError::Malformed);
    }
    Ok(records)
}

macro_rules! borrowed_poll_payload {
    ($wrapper:ident, $payload:ident, $schema:ident, $canonical:ident) => {
        impl WireEncode for $wrapper<'_> {
            fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
                encode(out, self.0.records, $canonical)
            }
        }
        impl WirePayload for $wrapper<'_> {
            const SCHEMA_ID: u32 = schema::$schema;
            type Decoded<'a> = $wrapper<'a>;
            fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
                $payload::new(decode(input.as_bytes(), $canonical)?).map(drop)
            }
            fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
                $wrapper(
                    $payload::new(
                        decode(input.as_bytes(), $canonical).expect("payload was validated"),
                    )
                    .expect("payload was validated"),
                )
            }
        }
    };
}

borrowed_poll_payload!(PollOneoffReq, PollOneoff, POLL_ONEOFF, subscription_fields);
borrowed_poll_payload!(PollReadyRet, PollReady, POLL_READY, event_fields);

#[cfg(test)]
mod tests {
    use super::*;
    const WASIP1_PENDING_BUDGET: usize = crate::protocol::WASIP1_IO_CHUNK_CAPACITY + 80;
    use crate::protocol::{PollOneoffReq, PollReadyRet};
    use hibana::runtime::wire::{Payload, WireEncode, WirePayload};

    fn subscription(kind: u8) -> [u8; 48] {
        let mut record = [0; 48];
        record[..8].copy_from_slice(&17u64.to_le_bytes());
        record[8] = kind;
        record[16..20].copy_from_slice(&1u32.to_le_bytes());
        if kind == 0 {
            record[24..32].copy_from_slice(&20u64.to_le_bytes());
            record[32..40].copy_from_slice(&3u64.to_le_bytes());
            record[40..42].copy_from_slice(&1u16.to_le_bytes());
        }
        record
    }

    #[test]
    fn subscriptions_preserve_clock_semantics_and_both_fd_interests() {
        let input = [subscription(0), subscription(1), subscription(2)];
        let request = PollOneoffReq(PollOneoff::new(&input).unwrap());
        assert_eq!(request.0.subscriptions(), input);
        let mut bytes = [0; 145];
        assert_eq!(request.encode_into(&mut bytes), Ok(145));
        assert_eq!(bytes[0], 3);
        PollOneoffReq::validate_payload(Payload::new(&bytes)).unwrap();
        assert_eq!(
            PollOneoffReq::decode_validated_payload(Payload::new(&bytes)),
            request
        );
        for capacity in 0..bytes.len() {
            let mut short = [0xa5; 145];
            assert_eq!(
                request.encode_into(&mut short[..capacity]),
                Err(CodecError::Truncated)
            );
            assert!(short.iter().all(|byte| *byte == 0xa5));
            assert!(PollOneoffReq::validate_payload(Payload::new(&bytes[..capacity])).is_err());
        }
    }

    #[test]
    fn subscriptions_reject_invalid_kind_clock_flags_and_fd_domain() {
        assert!(PollOneoff::new(&[]).is_err());
        assert!(PollOneoff::new(&[subscription(0); CAPACITY + 1]).is_err());
        let mut bad = subscription(3);
        assert!(PollOneoff::new(&[subscription(0), bad]).is_err());
        bad = subscription(0);
        bad[16..20].copy_from_slice(&4u32.to_le_bytes());
        assert!(PollOneoff::new(&[bad]).is_err());
        bad = subscription(0);
        bad[40..42].copy_from_slice(&2u16.to_le_bytes());
        assert!(PollOneoff::new(&[bad]).is_err());
        for kind in [1, 2] {
            bad = subscription(kind);
            bad[16..20].copy_from_slice(&256u32.to_le_bytes());
            assert!(PollOneoff::new(&[bad]).is_err());
        }
    }

    #[test]
    fn padding_is_canonical_and_never_grants_a_different_interest() {
        let mut input = subscription(1);
        input[24..].fill(0xa5);
        let inputs = [input];
        let request = PollOneoffReq(PollOneoff::new(&inputs).unwrap());
        assert_eq!(request.0.subscriptions(), &inputs);
        let mut wire = [0; 49];
        request.encode_into(&mut wire).unwrap();
        assert_eq!(&wire[1..], &subscription(1));
        assert_eq!(
            PollOneoffReq::decode_validated_payload(Payload::new(&wire)),
            request
        );
        wire[24] = 0xa5;
        assert_eq!(
            PollOneoffReq::validate_payload(Payload::new(&wire)),
            Err(CodecError::Malformed)
        );
    }

    #[test]
    fn events_are_nonempty_canonical_and_encode_atomically() {
        let mut event = [0; 32];
        event[..8].copy_from_slice(&17u64.to_le_bytes());
        event[10] = 1;
        event[16..24].copy_from_slice(&96u64.to_le_bytes());
        event[24..26].copy_from_slice(&1u16.to_le_bytes());
        let events = [event, event];
        let reply = PollReadyRet(PollReady::new(&events).unwrap());
        let mut wire = [0; 65];
        assert_eq!(reply.encode_into(&mut wire), Ok(65));
        PollReadyRet::validate_payload(Payload::new(&wire)).unwrap();
        assert_eq!(
            PollReadyRet::decode_validated_payload(Payload::new(&wire)),
            reply
        );
        for capacity in 0..wire.len() {
            let mut short = [0xa5; 65];
            assert_eq!(
                reply.encode_into(&mut short[..capacity]),
                Err(CodecError::Truncated)
            );
            assert!(short.iter().all(|byte| *byte == 0xa5));
        }
        assert!(PollReady::new(&[]).is_err());
        assert!(PollReady::new(&[event; CAPACITY + 1]).is_err());
        event[10] = 3;
        assert!(PollReady::new(&[event]).is_err());
        event[10] = 2;
        event[24..26].copy_from_slice(&2u16.to_le_bytes());
        assert!(PollReady::new(&[event]).is_err());
    }

    #[test]
    fn fixed_fd_domain_bounds_poll_storage() {
        assert_eq!(
            core::mem::size_of::<PollOneoff>(),
            core::mem::size_of::<&[[u8; 48]]>()
        );
        assert_eq!(
            core::mem::size_of::<PollReady>(),
            core::mem::size_of::<&[[u8; 32]]>()
        );
        assert!(core::mem::size_of::<crate::WasiImportPending>() <= WASIP1_PENDING_BUDGET);
        PollOneoff::new(&[subscription(0); CAPACITY]).unwrap();
    }
}

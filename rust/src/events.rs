//! Anchor's framing of CPI events.
//!
//! A program that emits its events with `emit_cpi!` (the adapter, its
//! forwarders) invokes itself once per event, with instruction data that is
//! Anchor's 8-byte event tag (`anchor_lang::event::EVENT_IX_TAG_LE`), the
//! event's 8-byte discriminator, and the event's Borsh body. Readers take a
//! transaction's inner instructions addressed to the program and pass each
//! one's data through [`cpi_event_discriminator`] to tell the event, and
//! through [`decode_cpi_event`] to read it as one of the program's generated
//! event types (for the adapter, `generated::protocol_adapter::events`).
//! Events never appear in the program log.

use anchor::event::EVENT_IX_TAG_LE;
use anchor::{AnchorDeserialize, Discriminator};

/// The length of an event discriminator: the 8 bytes after the event tag.
const DISCRIMINATOR_LEN: usize = 8;

/// Why instruction data is not one event.
#[derive(Debug)]
pub enum EventDecodeError {
    /// The data does not start with the event tag; it is not an event
    /// self-invocation.
    NotAnEvent,
    /// The data ends inside the event discriminator.
    Truncated,
    /// Bytes remain after the event body.
    TrailingBytes,
    /// The body is not a Borsh encoding of the event.
    Body(std::io::Error),
}

impl core::fmt::Display for EventDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EventDecodeError::NotAnEvent => {
                write!(f, "the instruction data does not start with the event tag")
            }
            EventDecodeError::Truncated => {
                write!(
                    f,
                    "the instruction data ends inside the event discriminator"
                )
            }
            EventDecodeError::TrailingBytes => write!(f, "bytes remain after the event body"),
            EventDecodeError::Body(e) => write!(f, "the event body does not decode: {e}"),
        }
    }
}

impl std::error::Error for EventDecodeError {}

/// The discriminator of the event in `data`, an event self-invocation's
/// instruction data.
pub fn cpi_event_discriminator(data: &[u8]) -> Result<[u8; 8], EventDecodeError> {
    let rest = data
        .strip_prefix(EVENT_IX_TAG_LE)
        .ok_or(EventDecodeError::NotAnEvent)?;
    rest.get(..DISCRIMINATOR_LEN)
        .ok_or(EventDecodeError::Truncated)?
        .try_into()
        .map_err(|_| EventDecodeError::Truncated)
}

/// The event `E` in `data`, an event self-invocation's instruction data, or
/// `None` when `data` holds another event. The body must be exactly one `E`.
pub fn decode_cpi_event<E: Discriminator + AnchorDeserialize>(
    data: &[u8],
) -> Result<Option<E>, EventDecodeError> {
    if cpi_event_discriminator(data)? != E::DISCRIMINATOR {
        return Ok(None);
    }
    let mut body = &data[EVENT_IX_TAG_LE.len() + DISCRIMINATOR_LEN..];
    let event = E::deserialize(&mut body).map_err(EventDecodeError::Body)?;
    if !body.is_empty() {
        return Err(EventDecodeError::TrailingBytes);
    }
    Ok(Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol_adapter::events::{LogicRefDenied, Upgraded};
    use anchor::AnchorSerialize;

    /// The instruction data of the event self-invocation emitting `event`.
    fn emitted<E: Discriminator + AnchorSerialize>(event: &E) -> Vec<u8> {
        let mut data = [EVENT_IX_TAG_LE, E::DISCRIMINATOR].concat();
        event.serialize(&mut data).unwrap();
        data
    }

    fn denied() -> LogicRefDenied {
        LogicRefDenied {
            logic_ref: [7; 32],
            consumed: true,
        }
    }

    #[test]
    fn the_event_tag_is_anchors() {
        // `anchor_lang::event::EVENT_IX_TAG`, sha256("anchor:event")[..8] as
        // a little-endian u64.
        assert_eq!(
            EVENT_IX_TAG_LE,
            [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d]
        );
    }

    #[test]
    fn an_event_decodes_as_its_type() {
        let data = emitted(&denied());
        println!("LogicRefDenied self-invocation data: {data:02x?}");
        assert_eq!(
            cpi_event_discriminator(&data).unwrap(),
            LogicRefDenied::DISCRIMINATOR
        );
        let decoded = decode_cpi_event::<LogicRefDenied>(&data)
            .unwrap()
            .expect("a LogicRefDenied");
        assert_eq!(decoded.logic_ref, [7; 32]);
        assert!(decoded.consumed);
    }

    #[test]
    fn another_events_data_is_none() {
        let data = emitted(&denied());
        let decoded = decode_cpi_event::<Upgraded>(&data).unwrap();
        assert!(decoded.is_none(), "LogicRefDenied read as {decoded:?}");
    }

    #[test]
    fn data_without_the_event_tag_is_not_an_event() {
        // An instruction's own data starts with its discriminator, not the tag.
        let data = emitted(&denied());
        for data in [&data[8..], &data[..4], &[][..]] {
            let result = decode_cpi_event::<LogicRefDenied>(data);
            assert!(
                matches!(result, Err(EventDecodeError::NotAnEvent)),
                "{data:02x?}: {result:?}"
            );
        }
    }

    #[test]
    fn data_ending_inside_the_discriminator_is_truncated() {
        let data = emitted(&denied());
        for len in [8, 15] {
            let result = cpi_event_discriminator(&data[..len]);
            assert!(
                matches!(result, Err(EventDecodeError::Truncated)),
                "{len} bytes: {result:?}"
            );
        }
    }

    #[test]
    fn a_short_body_does_not_decode() {
        let data = emitted(&denied());
        let result = decode_cpi_event::<LogicRefDenied>(&data[..data.len() - 1]);
        println!("short body: {result:?}");
        assert!(
            matches!(result, Err(EventDecodeError::Body(_))),
            "{result:?}"
        );
    }

    #[test]
    fn an_invalid_body_does_not_decode() {
        // A Borsh bool is 0 or 1.
        let mut data = emitted(&denied());
        *data.last_mut().unwrap() = 2;
        let result = decode_cpi_event::<LogicRefDenied>(&data);
        println!("bool byte 2: {result:?}");
        assert!(
            matches!(result, Err(EventDecodeError::Body(_))),
            "{result:?}"
        );
    }

    #[test]
    fn trailing_bytes_after_the_body_are_refused() {
        let mut data = emitted(&denied());
        data.push(0);
        let result = decode_cpi_event::<LogicRefDenied>(&data);
        assert!(
            matches!(result, Err(EventDecodeError::TrailingBytes)),
            "{result:?}"
        );
    }
}

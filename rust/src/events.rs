//! Decoders for the PA's settlement events.
//!
//! The PA emits every event as a self-invocation (Anchor `#[event_cpi]`): an
//! inner instruction whose program is the PA and whose data is the 8-byte
//! event tag, the event's 8-byte discriminator (`sha256("event:<Name>")[..8]`),
//! and the Borsh-encoded body. Readers take a settlement transaction's inner
//! instructions and pass each PA-addressed one through
//! [`decode_event_instruction`]. Events never appear in the program log.

use crate::constants::ANCHOR_DISCRIMINATOR_LEN;
use crate::cursor::{Cursor, Truncated};
use crate::discriminator::anchor_event_disc;

/// `anchor_lang::event::EVENT_IX_TAG_LE`: the u64 `0x1d9acb512ea545e4` little-endian.
pub const EVENT_IX_TAG: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

/// Body shared by the four payload events. `tag` is the resource tag the
/// payload belongs to; `index` is the entry's position within its category's
/// payload list for that resource; `blob` is the payload bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadEvent {
    pub tag: [u8; 32],
    pub index: u32,
    pub blob: Vec<u8>,
}

/// Emitted once per action, after that action's payload events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionExecutedEvent {
    pub action_tree_root: [u8; 32],
    pub action_tag_count: u32,
}

/// Emitted once per settlement, last. The three vectors are index-parallel;
/// `is_consumed[i]` is true when `tags[i]` is a nullifier and false when it is
/// a commitment. Tags are grouped per action (consumed, then created), so
/// index parity does not determine the role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionExecutedEvent {
    pub tags: Vec<[u8; 32]>,
    pub logic_refs: Vec<[u8; 32]>,
    pub is_consumed: Vec<bool>,
}

/// Emitted once per external call, in call order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwarderCallExecutedEvent {
    pub forwarder: [u8; 32],
    pub input: Vec<u8>,
    pub output: Vec<u8>,
}

/// One decoded PA event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaEvent {
    ResourcePayload(PayloadEvent),
    DiscoveryPayload(PayloadEvent),
    ExternalPayload(PayloadEvent),
    ApplicationPayload(PayloadEvent),
    ActionExecuted(ActionExecutedEvent),
    TransactionExecuted(TransactionExecutedEvent),
    ForwarderCallExecuted(ForwarderCallExecutedEvent),
}

/// Errors produced by [`decode_event_instruction`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventDecodeError {
    /// The data does not start with [`EVENT_IX_TAG`]; it is not an event self-invocation.
    NotAnEvent,
    /// The discriminator names no PA event.
    UnknownDiscriminator([u8; ANCHOR_DISCRIMINATOR_LEN]),
    /// The body ran out while reading the named field.
    Truncated { field: &'static str },
    /// A Borsh `bool` byte other than 0 or 1.
    InvalidBool(u8),
    /// Bytes remain after the event body.
    TrailingBytes(usize),
}

impl core::fmt::Display for EventDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EventDecodeError::NotAnEvent => {
                write!(
                    f,
                    "instruction data does not start with the Anchor event tag"
                )
            }
            EventDecodeError::UnknownDiscriminator(d) => {
                write!(f, "unknown event discriminator {}", hex(d))
            }
            EventDecodeError::Truncated { field } => {
                write!(f, "event truncated while reading {field}")
            }
            EventDecodeError::InvalidBool(b) => write!(f, "invalid bool byte {b}"),
            EventDecodeError::TrailingBytes(n) => {
                write!(f, "{n} trailing byte(s) after the event body")
            }
        }
    }
}

impl std::error::Error for EventDecodeError {}

impl From<Truncated> for EventDecodeError {
    fn from(t: Truncated) -> Self {
        EventDecodeError::Truncated { field: t.field }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decode the instruction data of one PA event self-invocation.
pub fn decode_event_instruction(data: &[u8]) -> Result<PaEvent, EventDecodeError> {
    let mut c = Cursor::new(data, 0);
    let tag = c.take(EVENT_IX_TAG.len(), "event tag");
    if tag != Ok(&EVENT_IX_TAG) {
        return Err(EventDecodeError::NotAnEvent);
    }
    let disc: [u8; ANCHOR_DISCRIMINATOR_LEN] = c
        .take(ANCHOR_DISCRIMINATOR_LEN, "event discriminator")?
        .try_into()
        .expect("8 bytes");

    let event = if disc == anchor_event_disc("ResourcePayloadEvent") {
        PaEvent::ResourcePayload(payload(&mut c)?)
    } else if disc == anchor_event_disc("DiscoveryPayloadEvent") {
        PaEvent::DiscoveryPayload(payload(&mut c)?)
    } else if disc == anchor_event_disc("ExternalPayloadEvent") {
        PaEvent::ExternalPayload(payload(&mut c)?)
    } else if disc == anchor_event_disc("ApplicationPayloadEvent") {
        PaEvent::ApplicationPayload(payload(&mut c)?)
    } else if disc == anchor_event_disc("ActionExecutedEvent") {
        PaEvent::ActionExecuted(ActionExecutedEvent {
            action_tree_root: c.array_32("action_tree_root")?,
            action_tag_count: c.u32_le("action_tag_count")?,
        })
    } else if disc == anchor_event_disc("TransactionExecutedEvent") {
        let tags = c.vec_array_32("tags")?;
        let logic_refs = c.vec_array_32("logic_refs")?;
        let len = c.u32_le("is_consumed")? as usize;
        let is_consumed = (0..len)
            .map(|_| match c.u8("is_consumed")? {
                0 => Ok(false),
                1 => Ok(true),
                b => Err(EventDecodeError::InvalidBool(b)),
            })
            .collect::<Result<Vec<bool>, EventDecodeError>>()?;
        PaEvent::TransactionExecuted(TransactionExecutedEvent {
            tags,
            logic_refs,
            is_consumed,
        })
    } else if disc == anchor_event_disc("ForwarderCallExecutedEvent") {
        PaEvent::ForwarderCallExecuted(ForwarderCallExecutedEvent {
            forwarder: c.array_32("forwarder")?,
            input: c.vec_u8("input")?,
            output: c.vec_u8("output")?,
        })
    } else {
        return Err(EventDecodeError::UnknownDiscriminator(disc));
    };

    match c.remaining() {
        0 => Ok(event),
        n => Err(EventDecodeError::TrailingBytes(n)),
    }
}

fn payload(c: &mut Cursor<'_>) -> Result<PayloadEvent, EventDecodeError> {
    Ok(PayloadEvent {
        tag: c.array_32("tag")?,
        index: c.u32_le("index")?,
        blob: c.vec_u8("blob")?,
    })
}

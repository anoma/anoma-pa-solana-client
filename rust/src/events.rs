//! Decoders for the PA's events.
//!
//! The PA emits every event as a self-invocation (Anchor `#[event_cpi]`): an
//! inner instruction whose program is the PA and whose data is the 8-byte
//! event tag, the event's 8-byte discriminator (`sha256("event:<Name>")[..8]`),
//! and the Borsh-encoded body. Readers take a settlement transaction's inner
//! instructions and pass each PA-addressed one through
//! [`decode_event_instruction`]. Events never appear in the program log.
//!
//! [`decode_cpi_event`] and the shared bodies ([`decode_ownership_transferred`],
//! [`decode_upgraded`]) serve any program that frames its events the same way.

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

/// Emitted once per action, after that action's resources are processed, as
/// pa-evm's `ActionExecuted`: the nullifiers of its consumed resources and the
/// commitments of its created ones, each with its resource's logic ref, in
/// instance order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionExecutedEvent {
    pub action_tree_root: [u8; 32],
    pub nullifiers: Vec<[u8; 32]>,
    pub consumed_logic_refs: Vec<[u8; 32]>,
    pub commitments: Vec<[u8; 32]>,
    pub created_logic_refs: Vec<[u8; 32]>,
}

/// Emitted once per settlement, last, as pa-evm's `TransactionExecuted`:
/// the transaction id is the Keccak-256 hash of the concatenated action tree
/// roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionExecutedEvent {
    pub transaction_id: [u8; 32],
}

/// A root the commitment tree took on: the empty tree's at initialization,
/// then the root of each settlement that appends commitments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitmentTreeRootAddedEvent {
    pub root: [u8; 32],
}

/// The kind-table commitment the adapter now requires: the empty table's at
/// initialization, then each `set_kind_table_commitment`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindTableCommitmentUpdatedEvent {
    pub kind_table_commitment: [u8; 32],
}

/// A logic ref the owner denied for good.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicRefDeniedEvent {
    pub logic_ref: [u8; 32],
}

/// The owner (`account`) paused or unpaused settlement, as OpenZeppelin
/// Pausable's `Paused(account)` / `Unpaused(account)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseEvent {
    pub account: [u8; 32],
}

/// The ownership moved from `previous_owner` to `new_owner`, as OpenZeppelin
/// Ownable's `OwnershipTransferred`; the zero key stands for no owner (the
/// previous owner at `initialize`, the new owner once
/// renounced). The PA emits it as `OwnershipTransferredEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnershipTransferredEvent {
    pub previous_owner: [u8; 32],
    pub new_owner: [u8; 32],
}

/// The owner upgraded the program to the code whose executable hash is
/// `executable_hash` (sha256 of the code without trailing zero bytes, what
/// `solana-verify get-program-hash` reports), as ERC1967's `Upgraded`. The PA
/// emits it as `UpgradedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradedEvent {
    pub executable_hash: [u8; 32],
}

/// Emitted once per external call, in call order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForwarderCallExecutedEvent {
    pub untrusted_forwarder: [u8; 32],
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
    CommitmentTreeRootAdded(CommitmentTreeRootAddedEvent),
    KindTableCommitmentUpdated(KindTableCommitmentUpdatedEvent),
    LogicRefDenied(LogicRefDeniedEvent),
    Paused(PauseEvent),
    Unpaused(PauseEvent),
    OwnershipTransferred(OwnershipTransferredEvent),
    Upgraded(UpgradedEvent),
}

/// Errors produced by [`decode_cpi_event`] and [`decode_event_instruction`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventDecodeError {
    /// The data does not start with [`EVENT_IX_TAG`]; it is not an event self-invocation.
    NotAnEvent,
    /// The discriminator names no event of the decoder's program.
    UnknownDiscriminator([u8; ANCHOR_DISCRIMINATOR_LEN]),
    /// The body ran out while reading the named field.
    Truncated { field: &'static str },
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

/// Decode one Anchor event self-invocation's data: check the tag, read the
/// discriminator, let `body` decode the body it names, and refuse trailing
/// bytes. Every program that emits its events with `emit_cpi!` (the adapter,
/// its forwarders) frames them this way.
pub fn decode_cpi_event<E>(
    data: &[u8],
    body: impl FnOnce([u8; ANCHOR_DISCRIMINATOR_LEN], &mut Cursor<'_>) -> Result<E, EventDecodeError>,
) -> Result<E, EventDecodeError> {
    let mut c = Cursor::new(data, 0);
    let tag = c.take(EVENT_IX_TAG.len(), "event tag");
    if tag != Ok(&EVENT_IX_TAG) {
        return Err(EventDecodeError::NotAnEvent);
    }
    let disc: [u8; ANCHOR_DISCRIMINATOR_LEN] = c
        .take(ANCHOR_DISCRIMINATOR_LEN, "event discriminator")?
        .try_into()
        .expect("8 bytes");
    let event = body(disc, &mut c)?;
    match c.remaining() {
        0 => Ok(event),
        n => Err(EventDecodeError::TrailingBytes(n)),
    }
}

/// Decode the instruction data of one PA event self-invocation.
pub fn decode_event_instruction(data: &[u8]) -> Result<PaEvent, EventDecodeError> {
    decode_cpi_event(data, pa_event_body)
}

fn pa_event_body(
    disc: [u8; ANCHOR_DISCRIMINATOR_LEN],
    c: &mut Cursor<'_>,
) -> Result<PaEvent, EventDecodeError> {
    Ok(if disc == anchor_event_disc("ResourcePayloadEvent") {
        PaEvent::ResourcePayload(payload(c)?)
    } else if disc == anchor_event_disc("DiscoveryPayloadEvent") {
        PaEvent::DiscoveryPayload(payload(c)?)
    } else if disc == anchor_event_disc("ExternalPayloadEvent") {
        PaEvent::ExternalPayload(payload(c)?)
    } else if disc == anchor_event_disc("ApplicationPayloadEvent") {
        PaEvent::ApplicationPayload(payload(c)?)
    } else if disc == anchor_event_disc("ActionExecutedEvent") {
        PaEvent::ActionExecuted(ActionExecutedEvent {
            action_tree_root: c.array_32("action_tree_root")?,
            nullifiers: c.vec_array_32("nullifiers")?,
            consumed_logic_refs: c.vec_array_32("consumed_logic_refs")?,
            commitments: c.vec_array_32("commitments")?,
            created_logic_refs: c.vec_array_32("created_logic_refs")?,
        })
    } else if disc == anchor_event_disc("TransactionExecutedEvent") {
        PaEvent::TransactionExecuted(TransactionExecutedEvent {
            transaction_id: c.array_32("transaction_id")?,
        })
    } else if disc == anchor_event_disc("CommitmentTreeRootAddedEvent") {
        PaEvent::CommitmentTreeRootAdded(CommitmentTreeRootAddedEvent {
            root: c.array_32("root")?,
        })
    } else if disc == anchor_event_disc("KindTableCommitmentUpdatedEvent") {
        PaEvent::KindTableCommitmentUpdated(KindTableCommitmentUpdatedEvent {
            kind_table_commitment: c.array_32("kind_table_commitment")?,
        })
    } else if disc == anchor_event_disc("LogicRefDeniedEvent") {
        PaEvent::LogicRefDenied(LogicRefDeniedEvent {
            logic_ref: c.array_32("logic_ref")?,
        })
    } else if disc == anchor_event_disc("PausedEvent") {
        PaEvent::Paused(PauseEvent {
            account: c.array_32("account")?,
        })
    } else if disc == anchor_event_disc("UnpausedEvent") {
        PaEvent::Unpaused(PauseEvent {
            account: c.array_32("account")?,
        })
    } else if disc == anchor_event_disc("OwnershipTransferredEvent") {
        PaEvent::OwnershipTransferred(decode_ownership_transferred(c)?)
    } else if disc == anchor_event_disc("UpgradedEvent") {
        PaEvent::Upgraded(decode_upgraded(c)?)
    } else if disc == anchor_event_disc("ForwarderCallExecutedEvent") {
        PaEvent::ForwarderCallExecuted(ForwarderCallExecutedEvent {
            untrusted_forwarder: c.array_32("untrusted_forwarder")?,
            input: c.vec_u8("input")?,
            output: c.vec_u8("output")?,
        })
    } else {
        return Err(EventDecodeError::UnknownDiscriminator(disc));
    })
}

/// The body of an `OwnershipTransferred` event, which the adapter and the
/// forwarders emit alike.
pub fn decode_ownership_transferred(
    c: &mut Cursor<'_>,
) -> Result<OwnershipTransferredEvent, EventDecodeError> {
    Ok(OwnershipTransferredEvent {
        previous_owner: c.array_32("previous_owner")?,
        new_owner: c.array_32("new_owner")?,
    })
}

/// The body of an `Upgraded` event, which the adapter and the forwarders emit
/// alike.
pub fn decode_upgraded(c: &mut Cursor<'_>) -> Result<UpgradedEvent, EventDecodeError> {
    Ok(UpgradedEvent {
        executable_hash: c.array_32("executable_hash")?,
    })
}

fn payload(c: &mut Cursor<'_>) -> Result<PayloadEvent, EventDecodeError> {
    Ok(PayloadEvent {
        tag: c.array_32("tag")?,
        index: c.u32_le("index")?,
        blob: c.vec_u8("blob")?,
    })
}

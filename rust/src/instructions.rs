//! Builders for the adapter's instructions, over the generated
//! `client::accounts` and `client::args` of `generated::protocol_adapter`.
//! Each takes the address the adapter is deployed at, which its build sets
//! (`declare_id!` from the environment), so one IDL serves every deployment.

use anchor::{InstructionData, ToAccountMetas};
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;
use solana_sdk_ids::{bpf_loader_upgradeable, system_program};

use crate::generated::protocol_adapter::client::{accounts, args};
use crate::generated::protocol_adapter::constants::{PA_STATE_SEED, UPGRADE_AUTHORITY_SEED};
use crate::generated::protocol_adapter::types::DeniedLogicRef;
use crate::generated::protocol_adapter::ID;
use crate::pda::{
    derive_event_authority_pda, derive_program_data_address, derive_verifier_entry_pda,
};

/// The instruction of the adapter at `pa_program` with `accounts`, then
/// `remaining_accounts`, and `args`.
///
/// The generated client marks an absent optional account by the IDL's
/// address, `ID`; Anchor reads an optional account as absent when it is the
/// address of the program it runs, `pa_program`. No other account is `ID`:
/// every builder names the program itself as `pa_program`.
fn instruction(
    pa_program: &Pubkey,
    accounts: impl ToAccountMetas,
    remaining_accounts: Vec<AccountMeta>,
    args: impl InstructionData,
) -> Instruction {
    let mut metas = accounts.to_account_metas(None);
    for meta in &mut metas {
        if meta.pubkey == ID {
            meta.pubkey = *pa_program;
        }
    }
    metas.extend(remaining_accounts);
    Instruction {
        program_id: *pa_program,
        accounts: metas,
        data: args.data(),
    }
}

/// The state account of the adapter at `pa_program`.
fn pa_state(pa_program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[PA_STATE_SEED], pa_program).0
}

/// Build the PA's `initialize`: `payer`, the program's upgrade authority, sets
/// the owner, verifier router and proof selector, and hands the upgrade
/// authority to the program.
pub fn initialize_ix(
    pa_program: &Pubkey,
    payer: &Pubkey,
    initial_owner: &Pubkey,
    verifier_router: &Pubkey,
    proof_selector: [u8; 4],
) -> Instruction {
    instruction(
        pa_program,
        accounts::Initialize {
            pa_state: pa_state(pa_program),
            payer: *payer,
            system_program: system_program::id(),
            program_data: derive_program_data_address(pa_program),
            upgrade_authority: Pubkey::find_program_address(&[UPGRADE_AUTHORITY_SEED], pa_program)
                .0,
            bpf_loader_upgradeable: bpf_loader_upgradeable::id(),
            verifier_entry: derive_verifier_entry_pda(verifier_router, proof_selector),
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::Initialize {
            initial_owner: *initial_owner,
            verifier_router: *verifier_router,
            proof_selector,
        },
    )
}

/// Build the PA's `pause`: the owner `authority` stops settlement until it
/// unpauses.
pub fn pause_ix(pa_program: &Pubkey, authority: &Pubkey) -> Instruction {
    instruction(
        pa_program,
        accounts::Pause {
            pa_state: pa_state(pa_program),
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::Pause {},
    )
}

/// Build the PA's `unpause`: the owner `authority` resumes settlement.
pub fn unpause_ix(pa_program: &Pubkey, authority: &Pubkey) -> Instruction {
    instruction(
        pa_program,
        accounts::Unpause {
            pa_state: pa_state(pa_program),
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::Unpause {},
    )
}

/// Build the PA's `deny_logic_refs`: the owner `authority` adds each entry
/// to its denylist and pays for the entries. A logic ref on the created side
/// only is deprecated (its resources are still consumed); one on both is
/// denied.
pub fn deny_logic_refs_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    logic_refs: &[DeniedLogicRef],
) -> Instruction {
    instruction(
        pa_program,
        accounts::DenyLogicRefs {
            pa_state: pa_state(pa_program),
            authority: *authority,
            system_program: system_program::id(),
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::DenyLogicRefs {
            logic_refs: logic_refs.to_vec(),
        },
    )
}

/// Build the PA's `set_kind_tree_root`: the owner `authority` replaces
/// the kind tree root settled transactions must be proven against.
pub fn set_kind_tree_root_ix(
    pa_program: &Pubkey,
    authority: &Pubkey,
    new_kind_tree_root: [u8; 32],
) -> Instruction {
    instruction(
        pa_program,
        accounts::SetKindTreeRoot {
            pa_state: pa_state(pa_program),
            authority: *authority,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        vec![],
        args::SetKindTreeRoot { new_kind_tree_root },
    )
}

/// Build a PA `txdata_init` instruction.
pub fn txdata_init_ix(
    pa_program: &Pubkey,
    pa_state: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    capacity: u32,
    expires_slot: u64,
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataInit {
            pa_state: *pa_state,
            tx_data: *tx_data,
            authority: *authority,
            system_program: system_program::id(),
        },
        vec![],
        args::TxdataInit {
            upload_id,
            capacity,
            expires_slot,
        },
    )
}

/// Build a PA `txdata_write` instruction for a single chunk.
///
/// `chunk.len()` is bounded by `TXDATA_CHUNK_SIZE`, the largest chunk whose
/// write transaction fits a v1 transaction.
pub fn txdata_write_ix(
    pa_program: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    offset: u32,
    chunk: &[u8],
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataWrite {
            tx_data: *tx_data,
            authority: *authority,
        },
        vec![],
        args::TxdataWrite {
            _upload_id: upload_id,
            offset,
            data: chunk.to_vec(),
        },
    )
}

/// Build a PA `txdata_close` instruction (reclaims rent from the upload PDA).
pub fn txdata_close_ix(
    pa_program: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    refund: &Pubkey,
    upload_id: u64,
) -> Instruction {
    instruction(
        pa_program,
        accounts::TxdataClose {
            tx_data: *tx_data,
            authority: *authority,
            refund: *refund,
        },
        vec![],
        args::TxdataClose { upload_id },
    )
}

/// The accounts every settlement instruction (`settle`, `simulate_execute`,
/// `settle_from_txdata`) names besides its payer and its upload, then its
/// remaining accounts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementAccounts {
    pub pa_state: Pubkey,
    /// The marker PDA of the root a settlement that creates commitments
    /// produces; `None` for one that creates nothing, which produces no root.
    pub new_root_marker: Option<Pubkey>,
    pub verifier_router_program: Pubkey,
    pub router: Pubkey,
    pub verifier_entry: Pubkey,
    pub verifier_program: Pubkey,
    /// The nullifier PDAs, each external call's CPI segment, and the
    /// historical root markers.
    pub remaining_accounts: Vec<AccountMeta>,
}

/// Build a PA `settle` instruction, which carries the settlement input
/// itself, paid for by `payer`.
pub fn settle_ix(
    pa_program: &Pubkey,
    payer: &Pubkey,
    input: &[u8],
    settlement: SettlementAccounts,
) -> Instruction {
    instruction(
        pa_program,
        accounts::Settle {
            pa_state: settlement.pa_state,
            payer: *payer,
            system_program: system_program::id(),
            new_root_marker: settlement.new_root_marker,
            verifier_router_program: settlement.verifier_router_program,
            router: settlement.router,
            verifier_entry: settlement.verifier_entry,
            verifier_program: settlement.verifier_program,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        settlement.remaining_accounts,
        args::Settle {
            transaction_data: input.to_vec(),
        },
    )
}

/// Build a PA `simulate_execute` instruction (pa-evm's `simulateExecute`):
/// `settle_ix`'s accounts and input, and whether to skip verifying the RISC
/// Zero proof. It always fails: with `Simulated` when the settlement would
/// execute, otherwise with the error `settle` would.
pub fn simulate_execute_ix(
    pa_program: &Pubkey,
    payer: &Pubkey,
    input: &[u8],
    skip_risc_zero_proof_verification: bool,
    settlement: SettlementAccounts,
) -> Instruction {
    instruction(
        pa_program,
        accounts::SimulateExecute {
            pa_state: settlement.pa_state,
            payer: *payer,
            system_program: system_program::id(),
            new_root_marker: settlement.new_root_marker,
            verifier_router_program: settlement.verifier_router_program,
            router: settlement.router,
            verifier_entry: settlement.verifier_entry,
            verifier_program: settlement.verifier_program,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        settlement.remaining_accounts,
        args::SimulateExecute {
            transaction_data: input.to_vec(),
            skip_risc_zero_proof_verification,
        },
    )
}

/// Build a PA `settle_from_txdata` instruction, which settles the input
/// `authority` uploaded to `tx_data` under `upload_id`.
pub fn settle_from_txdata_ix(
    pa_program: &Pubkey,
    tx_data: &Pubkey,
    authority: &Pubkey,
    upload_id: u64,
    settlement: SettlementAccounts,
) -> Instruction {
    instruction(
        pa_program,
        accounts::SettleFromTxdata {
            pa_state: settlement.pa_state,
            tx_data: *tx_data,
            authority: *authority,
            system_program: system_program::id(),
            new_root_marker: settlement.new_root_marker,
            verifier_router_program: settlement.verifier_router_program,
            router: settlement.router,
            verifier_entry: settlement.verifier_entry,
            verifier_program: settlement.verifier_program,
            event_authority: derive_event_authority_pda(pa_program).0,
            program: *pa_program,
        },
        settlement.remaining_accounts,
        args::SettleFromTxdata {
            _upload_id: upload_id,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol_adapter::constants::TX_DATA_SEED;

    /// What the hand-written builders, before the generated bindings replaced
    /// them, built for the inputs of `built()`: each instruction's program,
    /// data in hex, and accounts (address, signer, writable), in order.
    const HAND_WRITTEN: &str = "\
== initialize
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data afaf6d1f0d989bed02020202020202020202020202020202020202020202020202020202020202020303030303030303030303030303030303030303030303030303030303030303ffffffff
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct Hibe1esmNpASGJBynhevaMybmY2gcGcPDrfz8zMZyw1J false true
acct CFLMyAiswuYuTmzEmBT9b9C1T1V6xqvWEfy7DT1T8MEP false false
acct BPFLoaderUpgradeab1e11111111111111111111111 false false
acct AdJWkJknpP3onZQWvkzhfUfz6LxxwbDF1ohYmAV7NtK1 false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== pause
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data d316ddfb4a79c12f
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== unpause
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data a99004260a8dbcff
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== deny_logic_refs
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data 97c9411f1f8d54ca02000000070707070707070707070707070707070707070707070707070707070707070700080808080808080808080808080808080808080808080808080808080808080801
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true true
acct 11111111111111111111111111111111 false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== set_kind_tree_root
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data 61a25429e81390600707070707070707070707070707070707070707070707070707070707070707
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct GgBaCs3NCBuZN12kCJgAW63ydqohFkHEdfdEXBPzLHq true false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== settle
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data af2ab957908366d405000000696e707574
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
== settle_no_marker
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data af2ab957908366d405000000696e707574
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== simulate_execute_false
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data 11ba5ad41f0d004705000000696e70757400
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
== simulate_execute_true
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data 11ba5ad41f0d004705000000696e70757401
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
== settle_from_txdata
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data 8d611d6cf0fb0cbc0700000000000000
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false true
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false false
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
acct gBxS1f6uyyGPuW5MzGBukidSb71jdsCb5fZaoSzULE5 false true
acct k7FaK87WHGVXzkaoHb7CdVPgkKDQhZ29VLDeBVbDfYn false false
acct p2Yicb86aZig616Eav2VWG9vuXR5mEqhtzshZYBxzsV false false
acct swqrv48gsrwpBFbftEwnP2vB4jckpvfGJfXkwaniLCC false false
acct ws91DX9HBAAxGW77BZs5FogRDwpRtcUpiLBpKdPTfWu false false
acct BaNhKDjFJTKQhS1To1XK8eApMSYBoMUNky6wKrf4RX5N false false
acct cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN false false
acct 2MNus2KCpxwXnp19iyXNpWSFtBD2UGjQBAL8AbtywfT9 false true
acct 2RJD1KnDRGEkvuFfAGrJ7PD28LRE9LRDjZznDywagzmr false false
== txdata_init
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data 94ddddfb41d96ac7070000000000000084030000f401000000000000
acct Fb88t17apPNxTw4BCCC85yvcybdCvaY41qGJYpWXyrsR false false
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true true
acct 11111111111111111111111111111111 false false
== txdata_write
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data b0bef7915392e7c60700000000000000100f0000050000000102030405
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true false
== txdata_close
program cGfHiC6Kgg3FpFZvgwGcswsCRtp4aBP2fzuXRQPizuN
data a518a5860e0a6bac0700000000000000
acct DxJbRZapfMrinq8ofVVdfJZ3nmSG277N6wVwnbHdCzMK false true
acct 4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi true false
acct 8qbHbw2BbbTHBW1sbeqakYXVKRQM8Ne7pLK7m6CVfeR false true
";

    fn key(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// `ix` in `HAND_WRITTEN`'s form.
    fn written(name: &str, ix: &Instruction) -> String {
        let mut out = format!(
            "== {name}\nprogram {}\ndata {}\n",
            ix.program_id,
            hex(&ix.data)
        );
        for meta in &ix.accounts {
            out += &format!(
                "acct {} {} {}\n",
                meta.pubkey, meta.is_signer, meta.is_writable
            );
        }
        out
    }

    /// `HAND_WRITTEN`'s entry `name`.
    fn hand_written(name: &str) -> String {
        let start = HAND_WRITTEN
            .find(&format!("== {name}\n"))
            .unwrap_or_else(|| panic!("HAND_WRITTEN has no {name}"));
        let rest = &HAND_WRITTEN[start + 3..];
        let end = rest
            .find("== ")
            .map_or(HAND_WRITTEN.len(), |i| start + 3 + i);
        HAND_WRITTEN[start..end].to_string()
    }

    /// `ix` is what the hand-written builder built as `name`.
    fn assert_is_hand_written(name: &str, ix: &Instruction) {
        let built = written(name, ix);
        println!("{built}");
        assert_eq!(built, hand_written(name), "{name}");
    }

    /// The settlement accounts of `HAND_WRITTEN`'s settlements of `key(9)`.
    fn settlement(new_root_marker: Option<Pubkey>, remaining: bool) -> SettlementAccounts {
        SettlementAccounts {
            pa_state: pa_state(&key(9)),
            new_root_marker,
            verifier_router_program: key(11),
            router: key(12),
            verifier_entry: key(13),
            verifier_program: key(14),
            remaining_accounts: if remaining {
                vec![
                    AccountMeta::new(key(20), false),
                    AccountMeta::new_readonly(key(21), false),
                ]
            } else {
                vec![]
            },
        }
    }

    /// Checks `ix` against the adapter IDL's instruction `name`: the data is
    /// its discriminator then `args`, and each account has the IDL's flags,
    /// its fixed address, or its PDA from the IDL's own constant seeds.
    /// Returns the address `ix` passes for each IDL account name.
    fn assert_matches_the_adapters_idl(
        ix: &Instruction,
        name: &str,
        args: &[u8],
    ) -> impl Fn(&str) -> Pubkey {
        let idl: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/idls/protocol_adapter.json"
        )))
        .unwrap();
        let spec = idl["instructions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|spec| spec["name"] == name)
            .unwrap()
            .clone();

        let mut data: Vec<u8> = serde_json::from_value(spec["discriminator"].clone()).unwrap();
        data.extend(args);
        assert_eq!(
            ix.data, data,
            "{name}: the discriminator, then the arguments"
        );

        let accounts = spec["accounts"].as_array().unwrap().clone();
        assert!(ix.accounts.len() >= accounts.len(), "{name}: accounts");
        for (meta, account) in ix.accounts.iter().zip(&accounts) {
            let account_name = account["name"].as_str().unwrap();
            if account["optional"] == true && meta.pubkey == ix.program_id {
                continue;
            }
            assert_eq!(
                meta.is_writable,
                account["writable"] == true,
                "{account_name} writable"
            );
            assert_eq!(
                meta.is_signer,
                account["signer"] == true,
                "{account_name} signer"
            );
            if let Some(address) = account["address"].as_str() {
                assert_eq!(meta.pubkey.to_string(), address, "{account_name} address");
            }
            if let Some(seeds) = account["pda"]["seeds"].as_array() {
                if let Some(seeds) = seeds
                    .iter()
                    .map(|s| serde_json::from_value::<Vec<u8>>(s["value"].clone()).ok())
                    .collect::<Option<Vec<Vec<u8>>>>()
                {
                    let seeds: Vec<&[u8]> = seeds.iter().map(Vec::as_slice).collect();
                    assert_eq!(
                        meta.pubkey,
                        Pubkey::find_program_address(&seeds, &ix.program_id).0,
                        "{account_name} PDA"
                    );
                }
            }
        }
        let addresses = ix
            .accounts
            .iter()
            .map(|meta| meta.pubkey)
            .collect::<Vec<_>>();
        move |name: &str| {
            let i = accounts.iter().position(|a| a["name"] == name).unwrap();
            addresses[i]
        }
    }

    #[test]
    fn initialize_matches_the_adapters_idl() {
        assert_is_hand_written(
            "initialize",
            &initialize_ix(&key(9), &key(1), &key(2), &key(3), crate::MOCK_SELECTOR),
        );

        let (payer, owner, router) = (key(1), key(2), key(3));
        let selector = crate::MOCK_SELECTOR;
        let ix = initialize_ix(&ID, &payer, &owner, &router, selector);
        let args = [owner.to_bytes().as_slice(), &router.to_bytes(), &selector].concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "initialize", &args);
        assert_eq!(by_name("payer"), payer);
        assert_eq!(
            by_name("verifier_entry"),
            derive_verifier_entry_pda(&router, selector)
        );
        assert_eq!(
            by_name("event_authority"),
            derive_event_authority_pda(&ID).0
        );
        assert_eq!(by_name("program"), ID);
    }

    #[test]
    fn the_inline_settlement_instructions_match_the_adapters_idl() {
        let pa = key(9);
        assert_is_hand_written(
            "settle",
            &settle_ix(&pa, &key(1), b"input", settlement(Some(key(10)), true)),
        );
        // An absent new-root marker is the program's own address.
        assert_is_hand_written(
            "settle_no_marker",
            &settle_ix(&pa, &key(1), b"input", settlement(None, false)),
        );
        assert_is_hand_written(
            "simulate_execute_false",
            &simulate_execute_ix(
                &pa,
                &key(1),
                b"input",
                false,
                settlement(Some(key(10)), true),
            ),
        );
        assert_is_hand_written(
            "simulate_execute_true",
            &simulate_execute_ix(&pa, &key(1), b"input", true, settlement(None, false)),
        );

        let accounts = SettlementAccounts {
            pa_state: pa_state(&ID),
            ..settlement(Some(key(10)), false)
        };
        let input = b"input";
        let encoded_input = [&5u32.to_le_bytes()[..], input].concat();
        let settle = settle_ix(&ID, &key(1), input, accounts.clone());
        for skip in [false, true] {
            let simulate = simulate_execute_ix(&ID, &key(1), input, skip, accounts.clone());
            let args = [&encoded_input[..], &[u8::from(skip)]].concat();
            let by_name = assert_matches_the_adapters_idl(&simulate, "simulate_execute", &args);
            assert_eq!(by_name("payer"), key(1));
            assert_eq!(by_name("new_root_marker"), key(10));
            assert_eq!(by_name("verifier_program"), key(14));
            // The same accounts as `settle`.
            assert_eq!(simulate.accounts, settle.accounts);
        }
        let by_name = assert_matches_the_adapters_idl(&settle, "settle", &encoded_input);
        assert_eq!(by_name("new_root_marker"), key(10));
    }

    #[test]
    fn the_upload_instructions_match_the_adapters_idl() {
        let pa = key(9);
        let tx_data = |pa: &Pubkey| {
            Pubkey::find_program_address(&[TX_DATA_SEED, key(1).as_ref(), &7u64.to_le_bytes()], pa)
                .0
        };
        assert_is_hand_written(
            "settle_from_txdata",
            &settle_from_txdata_ix(
                &pa,
                &tx_data(&pa),
                &key(1),
                7,
                settlement(Some(key(10)), true),
            ),
        );
        assert_is_hand_written(
            "txdata_init",
            &txdata_init_ix(&pa, &pa_state(&pa), &tx_data(&pa), &key(1), 7, 900, 500),
        );
        assert_is_hand_written(
            "txdata_write",
            &txdata_write_ix(&pa, &tx_data(&pa), &key(1), 7, 3856, &[1, 2, 3, 4, 5]),
        );
        assert_is_hand_written(
            "txdata_close",
            &txdata_close_ix(&pa, &tx_data(&pa), &key(1), &key(2), 7),
        );

        let accounts = SettlementAccounts {
            pa_state: pa_state(&ID),
            ..settlement(None, false)
        };
        let ix = settle_from_txdata_ix(&ID, &tx_data(&ID), &key(1), 7, accounts);
        let by_name =
            assert_matches_the_adapters_idl(&ix, "settle_from_txdata", &7u64.to_le_bytes());
        assert_eq!(by_name("tx_data"), tx_data(&ID));
        assert_eq!(by_name("new_root_marker"), ID);
        let ix = txdata_init_ix(&ID, &pa_state(&ID), &tx_data(&ID), &key(1), 7, 900, 500);
        let args = [
            &7u64.to_le_bytes()[..],
            &900u32.to_le_bytes(),
            &500u64.to_le_bytes(),
        ]
        .concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_init", &args);
        assert_eq!(by_name("authority"), key(1), "txdata_init");
        let ix = txdata_write_ix(&ID, &tx_data(&ID), &key(1), 7, 3856, &[1, 2, 3, 4, 5]);
        let args = [
            &7u64.to_le_bytes()[..],
            &3856u32.to_le_bytes(),
            &5u32.to_le_bytes(),
            &[1, 2, 3, 4, 5],
        ]
        .concat();
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_write", &args);
        assert_eq!(by_name("authority"), key(1), "txdata_write");
        let ix = txdata_close_ix(&ID, &tx_data(&ID), &key(1), &key(2), 7);
        let by_name = assert_matches_the_adapters_idl(&ix, "txdata_close", &7u64.to_le_bytes());
        assert_eq!(by_name("refund"), key(2));
    }

    #[test]
    fn the_owner_instructions_match_the_adapters_idl() {
        let pa = key(9);
        let entries = [
            DeniedLogicRef {
                logic_ref: [7; 32],
                consumed: false,
            },
            DeniedLogicRef {
                logic_ref: [8; 32],
                consumed: true,
            },
        ];
        assert_is_hand_written("pause", &pause_ix(&pa, &key(4)));
        assert_is_hand_written("unpause", &unpause_ix(&pa, &key(4)));
        assert_is_hand_written(
            "deny_logic_refs",
            &deny_logic_refs_ix(&pa, &key(4), &entries),
        );
        assert_is_hand_written(
            "set_kind_tree_root",
            &set_kind_tree_root_ix(&pa, &key(4), [7; 32]),
        );

        let authority = key(4);
        let bytes = [7; 32];
        for (ix, name, args) in [
            (pause_ix(&ID, &authority), "pause", &[][..]),
            (unpause_ix(&ID, &authority), "unpause", &[][..]),
            (
                deny_logic_refs_ix(&ID, &authority, &entries[..1]),
                "deny_logic_refs",
                &[&[1, 0, 0, 0][..], &bytes, &[0]].concat()[..],
            ),
            (
                set_kind_tree_root_ix(&ID, &authority, bytes),
                "set_kind_tree_root",
                &bytes[..],
            ),
        ] {
            let by_name = assert_matches_the_adapters_idl(&ix, name, args);
            assert_eq!(by_name("authority"), authority, "{name}");
            assert_eq!(
                by_name("event_authority"),
                derive_event_authority_pda(&ID).0,
                "{name}"
            );
            assert_eq!(by_name("program"), ID, "{name}");
        }
    }
}

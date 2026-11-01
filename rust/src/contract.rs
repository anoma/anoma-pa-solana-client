//! The adapter deployed at an address, as pa-evm's bindings return it
//! (`contract::protocol_adapter`): its reads under their EVM names, from its
//! state and marker accounts, and its instructions, built for its address.

use anchor::AnchorDeserialize;
use base64::Engine;
use solana_instruction::Instruction;
use solana_message::{Hash, Message, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSimulateTransactionConfig;
use solana_rpc_client_api::response::{UiReturnDataEncoding, UiTransactionReturnData};
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;

use crate::accounts::decode_pa_state;
use crate::addresses::{clusters, protocol_adapter_address, Environment};
use crate::error::{rpc_transport_error, BindingsError, BindingsResult};
use crate::generated::protocol_adapter::accounts::PAStateAccount;
use crate::generated::protocol_adapter::constants::EMPTY_KIND_TREE_ROOT;
use crate::generated::protocol_adapter::types::DeniedLogicRef;
use crate::instructions::{self, SettlementAccounts};
use crate::merkle::{hash_two, PADDING_LEAF};
use crate::pda::{
    derive_nullifier_pda, derive_pa_state_pda, derive_root_marker_pda, derive_verifier_entry_pda,
};

/// Returns a protocol adapter instance of the environment for the given RPC
/// client.
pub async fn protocol_adapter(
    rpc: &RpcClient,
    environment: Environment,
) -> BindingsResult<ProtocolAdapterInstance<'_>> {
    let genesis_hash = rpc
        .get_genesis_hash()
        .await
        .map_err(rpc_transport_error("the genesis hash"))?;
    let address = recorded_address(environment, &genesis_hash)?;
    Ok(ProtocolAdapterInstance::new(address, rpc))
}

/// The adapter `environment` records on the cluster whose genesis hash is
/// `genesis_hash`.
fn recorded_address(environment: Environment, genesis_hash: &Hash) -> BindingsResult<Pubkey> {
    let cluster = clusters()
        .find(|cluster| cluster.get_genesis_hash().as_ref() == Some(genesis_hash))
        .ok_or(BindingsError::ChainIdUnknown)?;
    protocol_adapter_address(environment, &cluster).ok_or(BindingsError::UnsupportedChain(cluster))
}

/// The adapter at an address, read over an RPC at its commitment.
///
/// Its reads are exercised against a deployed adapter by the integration
/// tests of the adapter repository's harness, which deploys, initializes and
/// settles the adapter and reads it through `ProtocolAdapterInstance::new`.
pub struct ProtocolAdapterInstance<'a> {
    address: Pubkey,
    rpc: &'a RpcClient,
}

impl<'a> ProtocolAdapterInstance<'a> {
    /// The adapter at `address`, as alloy's `ProtocolAdapterInstance::new(address,
    /// provider)`.
    pub fn new(address: Pubkey, rpc: &'a RpcClient) -> Self {
        Self { address, rpc }
    }

    /// The address the adapter is deployed at.
    pub fn address(&self) -> Pubkey {
        self.address
    }

    fn pa_state(&self) -> Pubkey {
        derive_pa_state_pda(&self.address).0
    }

    async fn state(&self) -> BindingsResult<PAStateAccount> {
        let address = self.pa_state();
        let what = format!("the adapter's state {address}");
        let account = self
            .rpc
            .get_account_with_commitment(&address, self.rpc.commitment())
            .await
            .map_err(rpc_transport_error(&what))?
            .value
            .ok_or(BindingsError::AccountMissing { address })?;
        decode_pa_state(&account.data).map_err(|e| BindingsError::Decode {
            what,
            error: e.to_string(),
        })
    }

    /// Whether the account at `address` exists and the adapter owns it: a
    /// marker the adapter created.
    async fn is_marker(&self, address: Pubkey, what: &str) -> BindingsResult<bool> {
        let account = self
            .rpc
            .get_account_with_commitment(&address, self.rpc.commitment())
            .await
            .map_err(rpc_transport_error(&format!("{what} {address}")))?
            .value;
        Ok(account.is_some_and(|account| account.owner == self.address))
    }

    /// `paused()`.
    pub async fn paused(&self) -> BindingsResult<bool> {
        Ok(self.state().await?.paused)
    }

    /// `riscZeroVerifierPaused()`: the adapter's `risc_zero_verifier_paused`
    /// simulated, with `fee_payer`, any funded system account, paying the
    /// simulation's fee (nothing is signed or sent).
    pub async fn risc_zero_verifier_paused(&self, fee_payer: &Pubkey) -> BindingsResult<bool> {
        let state = self.state().await?;
        let ix = self.risc_zero_verifier_paused_ix(&state.verifier_router, state.proof_selector);
        let message = Message::new(&[ix], Some(fee_payer));
        let unsigned = VersionedTransaction {
            signatures: vec![
                Signature::default();
                usize::from(message.header.num_required_signatures)
            ],
            message: VersionedMessage::Legacy(message),
        };
        let simulated = self
            .rpc
            .simulate_transaction_with_config(
                &unsigned,
                RpcSimulateTransactionConfig {
                    sig_verify: false,
                    replace_recent_blockhash: true,
                    commitment: Some(self.rpc.commitment()),
                    ..RpcSimulateTransactionConfig::default()
                },
            )
            .await
            .map_err(rpc_transport_error(
                "the simulation of risc_zero_verifier_paused",
            ))?
            .value;
        if let Some(error) = simulated.err {
            return Err(BindingsError::Simulation {
                error: error.into(),
                logs: simulated.logs.unwrap_or_default(),
            });
        }
        returned::<bool>(&self.address, simulated.return_data)
    }

    /// `RISC_ZERO_VERIFIER_ROUTER()`.
    pub async fn risc_zero_verifier_router(&self) -> BindingsResult<Pubkey> {
        Ok(self.state().await?.verifier_router)
    }

    /// `RISC_ZERO_VERIFIER_SELECTOR()`.
    pub async fn risc_zero_verifier_selector(&self) -> BindingsResult<[u8; 4]> {
        Ok(self.state().await?.proof_selector)
    }

    /// `commitmentCount()`.
    pub async fn commitment_count(&self) -> BindingsResult<u64> {
        Ok(self.state().await?.next_index)
    }

    /// `commitmentTreeDepth()`.
    pub async fn commitment_tree_depth(&self) -> BindingsResult<u8> {
        Ok(self.state().await?.current_depth)
    }

    /// `commitmentTreeCapacity()`.
    pub async fn commitment_tree_capacity(&self) -> BindingsResult<u64> {
        Ok(capacity(&self.state().await?))
    }

    /// `commitmentTreeLeftSiblings()`, from the leaf level up.
    pub async fn commitment_tree_left_siblings(&self) -> BindingsResult<Vec<[u8; 32]>> {
        Ok(self.state().await?.frontier)
    }

    /// `commitmentTreeEmptySubtreeRoots()`, from the leaf level up.
    pub async fn commitment_tree_empty_subtree_roots(&self) -> BindingsResult<Vec<[u8; 32]>> {
        Ok(empty_subtree_roots(&self.state().await?))
    }

    /// `latestCommitmentTreeRoot()`.
    pub async fn latest_commitment_tree_root(&self) -> BindingsResult<[u8; 32]> {
        Ok(self.state().await?.root)
    }

    /// `isCommitmentTreeRootHistorical(root)`: `root` is the tree's initial
    /// root, the padding leaf, which has no marker, or the adapter holds
    /// `root`'s marker.
    pub async fn is_commitment_tree_root_historical(
        &self,
        root: &[u8; 32],
    ) -> BindingsResult<bool> {
        if *root == PADDING_LEAF {
            return Ok(true);
        }
        let marker = derive_root_marker_pda(&self.address, &self.pa_state(), root).0;
        self.is_marker(marker, "the root marker").await
    }

    /// `isNullifierContained(nullifier)`: the adapter holds `nullifier`'s
    /// marker.
    pub async fn is_nullifier_contained(&self, nullifier: &[u8; 32]) -> BindingsResult<bool> {
        let marker = derive_nullifier_pda(&self.address, &self.pa_state(), nullifier).0;
        self.is_marker(marker, "the nullifier marker").await
    }

    /// `getKindTreeRoot()`.
    pub async fn get_kind_tree_root(&self) -> BindingsResult<[u8; 32]> {
        Ok(self.state().await?.kind_tree_root)
    }

    /// `EMPTY_KIND_TREE_ROOT()`.
    pub fn empty_kind_tree_root(&self) -> [u8; 32] {
        EMPTY_KIND_TREE_ROOT
    }

    /// `isLogicRefDenied(logicRef, consumed)`.
    pub async fn is_logic_ref_denied(
        &self,
        logic_ref: &[u8; 32],
        consumed: bool,
    ) -> BindingsResult<bool> {
        Ok(is_logic_ref_denied(
            &self.state().await?,
            logic_ref,
            consumed,
        ))
    }

    /// `owner()`: the zero key once the ownership is renounced.
    pub async fn owner(&self) -> BindingsResult<Pubkey> {
        Ok(self.state().await?.owner)
    }

    /// `initialize`, with `payer` the program's upgrade authority.
    pub fn initialize_ix(
        &self,
        payer: &Pubkey,
        initial_owner: &Pubkey,
        verifier_router: &Pubkey,
        proof_selector: [u8; 4],
    ) -> Instruction {
        instructions::initialize_ix(
            &self.address,
            payer,
            initial_owner,
            verifier_router,
            proof_selector,
        )
    }

    /// `pause`, by the owner `authority`.
    pub fn pause_ix(&self, authority: &Pubkey) -> Instruction {
        instructions::pause_ix(&self.address, authority)
    }

    /// `unpause`, by the owner `authority`.
    pub fn unpause_ix(&self, authority: &Pubkey) -> Instruction {
        instructions::unpause_ix(&self.address, authority)
    }

    /// `deny_logic_refs`, by the owner `authority`.
    pub fn deny_logic_refs_ix(
        &self,
        authority: &Pubkey,
        logic_refs: &[DeniedLogicRef],
    ) -> Instruction {
        instructions::deny_logic_refs_ix(&self.address, authority, logic_refs)
    }

    /// `set_kind_tree_root`, by the owner `authority`.
    pub fn set_kind_tree_root_ix(
        &self,
        authority: &Pubkey,
        new_kind_tree_root: [u8; 32],
    ) -> Instruction {
        instructions::set_kind_tree_root_ix(&self.address, authority, new_kind_tree_root)
    }

    /// `transfer_ownership`, by the owner `authority`.
    pub fn transfer_ownership_ix(&self, authority: &Pubkey, new_owner: &Pubkey) -> Instruction {
        instructions::transfer_ownership_ix(&self.address, authority, new_owner)
    }

    /// `renounce_ownership`, by the owner `authority`.
    pub fn renounce_ownership_ix(&self, authority: &Pubkey) -> Instruction {
        instructions::renounce_ownership_ix(&self.address, authority)
    }

    /// `update_expiry_config`, by the owner `authority`.
    pub fn update_expiry_config_ix(
        &self,
        authority: &Pubkey,
        new_min_expiry_slots: u64,
        new_max_expiry_slots: u64,
    ) -> Instruction {
        instructions::update_expiry_config_ix(
            &self.address,
            authority,
            new_min_expiry_slots,
            new_max_expiry_slots,
        )
    }

    /// `upgrade` to `buffer`'s code, by the owner `authority`.
    pub fn upgrade_ix(&self, authority: &Pubkey, buffer: &Pubkey, spill: &Pubkey) -> Instruction {
        instructions::upgrade_ix(&self.address, authority, buffer, spill)
    }

    /// `risc_zero_verifier_paused`, for the deployment's `verifier_router`
    /// and `proof_selector`.
    pub fn risc_zero_verifier_paused_ix(
        &self,
        verifier_router: &Pubkey,
        proof_selector: [u8; 4],
    ) -> Instruction {
        instructions::risc_zero_verifier_paused_ix(
            &self.address,
            &derive_verifier_entry_pda(verifier_router, proof_selector),
        )
    }

    /// `version`.
    pub fn version_ix(&self) -> Instruction {
        instructions::version_ix(&self.address)
    }

    /// `txdata_init` of `authority`'s upload `upload_id`.
    pub fn txdata_init_ix(
        &self,
        authority: &Pubkey,
        upload_id: u64,
        capacity: u32,
        expires_slot: u64,
    ) -> Instruction {
        instructions::txdata_init_ix(&self.address, authority, upload_id, capacity, expires_slot)
    }

    /// `txdata_write` of `chunk` at `offset` into `authority`'s upload
    /// `upload_id`.
    pub fn txdata_write_ix(
        &self,
        authority: &Pubkey,
        upload_id: u64,
        offset: u32,
        chunk: &[u8],
    ) -> Instruction {
        instructions::txdata_write_ix(&self.address, authority, upload_id, offset, chunk)
    }

    /// `txdata_extend` of `authority`'s upload `upload_id`.
    pub fn txdata_extend_ix(
        &self,
        authority: &Pubkey,
        upload_id: u64,
        new_expires_slot: u64,
    ) -> Instruction {
        instructions::txdata_extend_ix(&self.address, authority, upload_id, new_expires_slot)
    }

    /// `txdata_close` of `authority`'s upload `upload_id`, its rent going to
    /// `refund`.
    pub fn txdata_close_ix(
        &self,
        authority: &Pubkey,
        refund: &Pubkey,
        upload_id: u64,
    ) -> Instruction {
        instructions::txdata_close_ix(&self.address, authority, refund, upload_id)
    }

    /// `txdata_close_expired` of `authority`'s upload `upload_id`, by
    /// `payer`, its rent going to `refund`.
    pub fn txdata_close_expired_ix(
        &self,
        payer: &Pubkey,
        refund: &Pubkey,
        authority: &Pubkey,
        upload_id: u64,
    ) -> Instruction {
        instructions::txdata_close_expired_ix(&self.address, payer, refund, upload_id, authority)
    }

    /// `settle` of `input`, paid for by `payer`.
    pub fn settle_ix(
        &self,
        payer: &Pubkey,
        input: &[u8],
        settlement: SettlementAccounts,
    ) -> Instruction {
        instructions::settle_ix(&self.address, payer, input, settlement)
    }

    /// `simulate_execute` of `input` (pa-evm's `simulateExecute`).
    pub fn simulate_execute_ix(
        &self,
        payer: &Pubkey,
        input: &[u8],
        skip_risc_zero_proof_verification: bool,
        settlement: SettlementAccounts,
    ) -> Instruction {
        instructions::simulate_execute_ix(
            &self.address,
            payer,
            input,
            skip_risc_zero_proof_verification,
            settlement,
        )
    }

    /// `settle_from_txdata` of `authority`'s upload `upload_id`.
    pub fn settle_from_txdata_ix(
        &self,
        authority: &Pubkey,
        upload_id: u64,
        settlement: SettlementAccounts,
    ) -> Instruction {
        instructions::settle_from_txdata_ix(&self.address, authority, upload_id, settlement)
    }
}

/// `commitmentTreeCapacity()` of `state`: two to the power of its depth.
fn capacity(state: &PAStateAccount) -> u64 {
    1 << state.current_depth
}

/// `commitmentTreeEmptySubtreeRoots()` of `state`: the root of an empty
/// subtree of each height from 0 (the padding leaf) to the tree's depth.
fn empty_subtree_roots(state: &PAStateAccount) -> Vec<[u8; 32]> {
    std::iter::successors(Some(PADDING_LEAF), |below| Some(hash_two(below, below)))
        .take(usize::from(state.current_depth) + 1)
        .collect()
}

/// `isLogicRefDenied(logicRef, consumed)` of `state`: the denylist of the
/// side `consumed` names holds `logic_ref`.
fn is_logic_ref_denied(state: &PAStateAccount, logic_ref: &[u8; 32], consumed: bool) -> bool {
    if consumed {
        state.denied_consumed_logic_refs.contains(logic_ref)
    } else {
        state.denied_created_logic_refs.contains(logic_ref)
    }
}

/// The value of type `T` that `program` returned, as a simulation reports
/// it.
fn returned<T: AnchorDeserialize>(
    program: &Pubkey,
    return_data: Option<UiTransactionReturnData>,
) -> BindingsResult<T> {
    let what = format!("the return data of {program}");
    let decode = |error: String| BindingsError::Decode {
        what: what.clone(),
        error,
    };
    let data = return_data.ok_or_else(|| decode("the simulation returned nothing".to_owned()))?;
    if data.program_id != program.to_string() {
        return Err(decode(format!("{} returned it", data.program_id)));
    }
    let (encoded, UiReturnDataEncoding::Base64) = &data.data;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| decode(e.to_string()))?;
    T::try_from_slice(&bytes).map_err(|e| decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::protocol_adapter::constants::SCHEMA_VERSION;
    use crate::merkle::{zero_hashes, CommitmentTreeState};
    use crate::{MAX_TREE_DEPTH, MOCK_SELECTOR};
    use solana_cluster_type::ClusterType;

    fn key(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn genesis_hash(cluster: ClusterType) -> Hash {
        cluster.get_genesis_hash().unwrap()
    }

    #[test]
    fn the_recorded_staging_devnet_adapter_is_the_idls_program() {
        let found = recorded_address(Environment::Staging, &genesis_hash(ClusterType::Devnet));
        println!("{found:?}");
        assert!(
            matches!(found, Ok(address) if address == crate::generated::protocol_adapter::ID),
            "{found:?}"
        );
    }

    #[test]
    fn production_records_nothing_on_mainnet_beta() {
        let found = recorded_address(
            Environment::Production,
            &genesis_hash(ClusterType::MainnetBeta),
        );
        println!("{found:?}");
        assert!(
            matches!(
                found,
                Err(BindingsError::UnsupportedChain(ClusterType::MainnetBeta))
            ),
            "{found:?}"
        );
    }

    /// A genesis hash of no known cluster, such as a local validator's, is
    /// `ChainIdUnknown` in every environment.
    #[test]
    fn a_genesis_hash_of_no_known_cluster_is_chain_id_unknown() {
        let unknown = Hash::new_from_array([0; 32]);
        for environment in [Environment::Staging, Environment::Production] {
            let found = recorded_address(environment, &unknown);
            println!("{environment:?}: {found:?}");
            assert!(
                matches!(found, Err(BindingsError::ChainIdUnknown)),
                "{found:?}"
            );
        }
    }

    /// The state `initialize` writes, as the program's `PAStateAccount::running`
    /// writes it, then `leaves` appended.
    fn state(leaves: &[[u8; 32]]) -> PAStateAccount {
        let tree = CommitmentTreeState::over(leaves).unwrap();
        PAStateAccount {
            schema_version: SCHEMA_VERSION,
            bump: 255,
            owner: key(1),
            verifier_router: key(2),
            proof_selector: MOCK_SELECTOR,
            kind_tree_root: EMPTY_KIND_TREE_ROOT,
            paused: false,
            root: tree.root,
            next_index: tree.next_index,
            current_depth: tree.current_depth,
            frontier: tree.frontier,
            min_expiry_slots: 100,
            max_expiry_slots: 216_000,
            denied_consumed_logic_refs: vec![[0xDD; 32]],
            denied_created_logic_refs: vec![[0xEE; 32], [0xDD; 32]],
        }
    }

    #[test]
    fn the_capacity_is_two_to_the_depth() {
        for count in [0, 1, 2, 3, 4, 5, 8] {
            let leaves: Vec<[u8; 32]> = (0..count).map(|i| [i as u8 + 1; 32]).collect();
            let state = state(&leaves);
            println!(
                "{count} leaves: depth {}, capacity {}",
                state.current_depth,
                capacity(&state)
            );
            assert_eq!(capacity(&state), 1u64 << state.current_depth);
            assert!(capacity(&state) > state.next_index, "{count} leaves");
        }
    }

    /// Each height's empty subtree root, up to the depth: the padding leaf,
    /// then each the hash of the one below with itself, as the zero hashes
    /// the tree pads with.
    #[test]
    fn the_empty_subtree_roots_reach_the_depth() {
        let zeros = zero_hashes();
        for count in [0, 1, 2, 5] {
            let leaves: Vec<[u8; 32]> = (0..count).map(|i| [i as u8 + 1; 32]).collect();
            let state = state(&leaves);
            let roots = empty_subtree_roots(&state);
            println!(
                "{count} leaves, depth {}: {} roots",
                state.current_depth,
                roots.len()
            );
            assert_eq!(roots.len(), usize::from(state.current_depth) + 1);
            assert_eq!(roots, zeros[..roots.len()]);
        }
        let deepest = PAStateAccount {
            current_depth: MAX_TREE_DEPTH as u8,
            ..state(&[])
        };
        let roots = empty_subtree_roots(&deepest);
        assert_eq!(roots.len(), MAX_TREE_DEPTH + 1);
        assert_eq!(roots[..MAX_TREE_DEPTH], zeros[..]);
        assert_eq!(
            roots[MAX_TREE_DEPTH],
            hash_two(&zeros[MAX_TREE_DEPTH - 1], &zeros[MAX_TREE_DEPTH - 1])
        );
    }

    #[test]
    fn a_logic_ref_is_denied_on_the_side_whose_list_holds_it() {
        let state = state(&[]);
        for (logic_ref, consumed, denied) in [
            ([0xDD; 32], true, true),
            ([0xDD; 32], false, true),
            ([0xEE; 32], true, false),
            ([0xEE; 32], false, true),
            ([0x01; 32], true, false),
            ([0x01; 32], false, false),
        ] {
            assert_eq!(
                is_logic_ref_denied(&state, &logic_ref, consumed),
                denied,
                "{:02x} consumed {consumed}",
                logic_ref[0]
            );
        }
    }

    fn return_data(program: &Pubkey, bytes: &[u8]) -> Option<UiTransactionReturnData> {
        Some(UiTransactionReturnData {
            program_id: program.to_string(),
            data: (
                base64::engine::general_purpose::STANDARD.encode(bytes),
                UiReturnDataEncoding::Base64,
            ),
        })
    }

    #[test]
    fn the_returned_bool_is_the_programs() {
        let program = key(9);
        assert!(returned::<bool>(&program, return_data(&program, &[1])).unwrap());
        assert!(!returned::<bool>(&program, return_data(&program, &[0])).unwrap());
        for (what, data) in [
            ("nothing", None),
            ("another program's", return_data(&key(8), &[1])),
            ("a byte 2", return_data(&program, &[2])),
            ("two bytes", return_data(&program, &[1, 0])),
            ("no bytes", return_data(&program, &[])),
        ] {
            let result = returned::<bool>(&program, data);
            println!("{what}: {result:?}");
            assert!(
                matches!(result, Err(BindingsError::Decode { .. })),
                "{what}: {result:?}"
            );
        }
    }

    /// Each builder is the module builder at the instance's address, with the
    /// upload and verifier entry it derives; there is one for every
    /// instruction the IDL declares, and each builds that instruction.
    #[test]
    fn the_builders_build_at_the_instances_address() {
        let idl: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/idls/protocol_adapter.json"
        )))
        .unwrap();
        let idl_discriminators: std::collections::BTreeMap<String, Vec<u8>> = idl["instructions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|ix| {
                (
                    ix["name"].as_str().unwrap().to_owned(),
                    serde_json::from_value(ix["discriminator"].clone()).unwrap(),
                )
            })
            .collect();
        let mut built_names = std::collections::BTreeSet::new();
        let rpc = RpcClient::new("http://127.0.0.1:1".to_owned());
        let pa = key(9);
        let adapter = ProtocolAdapterInstance::new(pa, &rpc);
        assert_eq!(adapter.address(), pa);
        let settlement = SettlementAccounts {
            new_root_marker: None,
            verifier_router_program: key(11),
            router: key(12),
            verifier_entry: key(13),
            verifier_program: key(14),
            remaining_accounts: vec![],
        };
        let entries = [DeniedLogicRef {
            logic_ref: [7; 32],
            consumed: true,
        }];
        for (name, built, expected) in [
            (
                "initialize",
                adapter.initialize_ix(&key(1), &key(2), &key(3), MOCK_SELECTOR),
                instructions::initialize_ix(&pa, &key(1), &key(2), &key(3), MOCK_SELECTOR),
            ),
            (
                "pause",
                adapter.pause_ix(&key(4)),
                instructions::pause_ix(&pa, &key(4)),
            ),
            (
                "unpause",
                adapter.unpause_ix(&key(4)),
                instructions::unpause_ix(&pa, &key(4)),
            ),
            (
                "deny_logic_refs",
                adapter.deny_logic_refs_ix(&key(4), &entries),
                instructions::deny_logic_refs_ix(&pa, &key(4), &entries),
            ),
            (
                "set_kind_tree_root",
                adapter.set_kind_tree_root_ix(&key(4), [7; 32]),
                instructions::set_kind_tree_root_ix(&pa, &key(4), [7; 32]),
            ),
            (
                "transfer_ownership",
                adapter.transfer_ownership_ix(&key(4), &key(5)),
                instructions::transfer_ownership_ix(&pa, &key(4), &key(5)),
            ),
            (
                "renounce_ownership",
                adapter.renounce_ownership_ix(&key(4)),
                instructions::renounce_ownership_ix(&pa, &key(4)),
            ),
            (
                "update_expiry_config",
                adapter.update_expiry_config_ix(&key(4), 100, 200),
                instructions::update_expiry_config_ix(&pa, &key(4), 100, 200),
            ),
            (
                "upgrade",
                adapter.upgrade_ix(&key(4), &key(6), &key(7)),
                instructions::upgrade_ix(&pa, &key(4), &key(6), &key(7)),
            ),
            (
                "risc_zero_verifier_paused",
                adapter.risc_zero_verifier_paused_ix(&key(3), MOCK_SELECTOR),
                instructions::risc_zero_verifier_paused_ix(
                    &pa,
                    &derive_verifier_entry_pda(&key(3), MOCK_SELECTOR),
                ),
            ),
            (
                "version",
                adapter.version_ix(),
                instructions::version_ix(&pa),
            ),
            (
                "txdata_init",
                adapter.txdata_init_ix(&key(1), 7, 900, 500),
                instructions::txdata_init_ix(&pa, &key(1), 7, 900, 500),
            ),
            (
                "txdata_write",
                adapter.txdata_write_ix(&key(1), 7, 3, &[1, 2]),
                instructions::txdata_write_ix(&pa, &key(1), 7, 3, &[1, 2]),
            ),
            (
                "txdata_extend",
                adapter.txdata_extend_ix(&key(1), 7, 900),
                instructions::txdata_extend_ix(&pa, &key(1), 7, 900),
            ),
            (
                "txdata_close",
                adapter.txdata_close_ix(&key(1), &key(2), 7),
                instructions::txdata_close_ix(&pa, &key(1), &key(2), 7),
            ),
            (
                "txdata_close_expired",
                adapter.txdata_close_expired_ix(&key(2), &key(3), &key(1), 7),
                instructions::txdata_close_expired_ix(&pa, &key(2), &key(3), 7, &key(1)),
            ),
            (
                "settle",
                adapter.settle_ix(&key(1), b"input", settlement.clone()),
                instructions::settle_ix(&pa, &key(1), b"input", settlement.clone()),
            ),
            (
                "simulate_execute",
                adapter.simulate_execute_ix(&key(1), b"input", true, settlement.clone()),
                instructions::simulate_execute_ix(&pa, &key(1), b"input", true, settlement.clone()),
            ),
            (
                "settle_from_txdata",
                adapter.settle_from_txdata_ix(&key(1), 7, settlement.clone()),
                instructions::settle_from_txdata_ix(&pa, &key(1), 7, settlement.clone()),
            ),
        ] {
            println!("{name}: {} accounts", built.accounts.len());
            assert_eq!(built, expected, "{name}");
            let discriminator = idl_discriminators
                .get(name)
                .unwrap_or_else(|| panic!("the IDL declares no instruction {name}"));
            assert!(
                built.data.starts_with(discriminator),
                "the {name} builder's data {:02x?} does not start with its IDL discriminator {discriminator:02x?}",
                built.data
            );
            assert!(built_names.insert(name), "{name} is built twice");
        }
        let idl_names: std::collections::BTreeSet<&str> =
            idl_discriminators.keys().map(String::as_str).collect();
        assert_eq!(
            built_names, idl_names,
            "the instance's builders and the IDL's instructions differ"
        );
    }

    /// The tree's initial root, the padding leaf, is historical from
    /// `initialize` on (pa-evm's initializer adds it to the root set; the
    /// adapter accepts it with no marker), so answering needs no account.
    /// Nothing listens on port 1: any other root's read fails with
    /// `RpcTransportError`.
    #[tokio::test]
    async fn the_initial_root_is_historical_without_a_marker() {
        let rpc = RpcClient::new("http://127.0.0.1:1".to_string());
        let adapter = ProtocolAdapterInstance::new(key(9), &rpc);
        let initial = adapter
            .is_commitment_tree_root_historical(&PADDING_LEAF)
            .await;
        println!("initial root: {initial:?}");
        assert!(
            matches!(initial, Ok(true)),
            "the padding leaf must be historical without reading its marker, got {initial:?}"
        );
        let other = adapter
            .is_commitment_tree_root_historical(&[0x99; 32])
            .await;
        println!("another root: {other:?}");
        assert!(
            matches!(other, Err(BindingsError::RpcTransportError { .. })),
            "any other root is read from its marker, got {other:?}"
        );
    }
}

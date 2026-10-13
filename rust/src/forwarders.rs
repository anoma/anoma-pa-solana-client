//! The forwarders a protocol adapter's settlements call. The proof commits
//! each external call's program, instruction data, expected output and
//! account count, but not the accounts: those are the submitter's to supply,
//! so a submitter registers, for each forwarder program its transactions call,
//! how it passes that forwarder its calls' accounts. Each forwarder's own
//! client implements [`Forwarder`]; nothing here knows any one forwarder.
//!
//! A forwarder is asked once per settlement, for all its calls: what precedes
//! the settlement can depend on all of them, as when each of a forwarder's
//! calls names the absolute index of an instruction it needs.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use solana_instruction::AccountMeta;
use solana_pubkey::Pubkey;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;

use crate::external_call::SolanaExternalCall;
use crate::settlement::CallAccounts;

/// Why a forwarder could not give its calls' accounts.
pub type ForwarderError = Box<dyn std::error::Error + Send + Sync>;

/// The accounts a forwarder gives, once it has read what it needs.
pub type CallAccountsFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CallAccounts, ForwarderError>> + Send + 'a>>;

/// How a submitter supplies a forwarder's calls with their accounts.
pub trait Forwarder: Send + Sync {
    /// The accounts a settlement passes this forwarder for `calls`, all of
    /// the settlement's calls to it in call order, as the chain behind `rpc`
    /// stands. The settlement transaction carries this forwarder's preceding
    /// instructions from index `preceding_start` on, which is where a call
    /// naming the absolute index of one finds it.
    fn call_accounts<'a>(
        &'a self,
        rpc: &'a RpcClient,
        calls: &'a [SolanaExternalCall],
        preceding_start: usize,
    ) -> CallAccountsFuture<'a>;
}

/// Why a settlement's calls have no accounts.
#[derive(Debug)]
pub enum AccountsError {
    /// A call is to a program no forwarder is registered for.
    Unregistered { call: usize, program: Pubkey },
    /// The forwarder failed to give its calls' accounts.
    Forwarder {
        calls: Vec<usize>,
        program: Pubkey,
        error: ForwarderError,
    },
    /// The forwarder gave another number of segments than it has calls.
    SegmentCount {
        program: Pubkey,
        segments: usize,
        calls: usize,
    },
    /// A segment's length is not the account count the proof commits.
    AccountCount {
        call: usize,
        program: Pubkey,
        accounts: usize,
        committed: u8,
    },
    /// A segment does not start with the forwarder program.
    NotLedByForwarder {
        call: usize,
        program: Pubkey,
        first: Option<Pubkey>,
    },
}

impl fmt::Display for AccountsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unregistered { call, program } => write!(
                f,
                "external call {call} is to {program}, for which no forwarder is registered"
            ),
            Self::Forwarder {
                calls,
                program,
                error,
            } => write!(
                f,
                "the accounts of external calls {calls:?} to {program}: {error}"
            ),
            Self::SegmentCount {
                program,
                segments,
                calls,
            } => write!(
                f,
                "the forwarder {program} gives {segments} segments for its {calls} calls"
            ),
            Self::AccountCount {
                call,
                program,
                accounts,
                committed,
            } => write!(
                f,
                "the forwarder {program} gives external call {call} {accounts} accounts; the \
                 proof commits {committed}"
            ),
            Self::NotLedByForwarder {
                call,
                program,
                first: None,
            } => write!(
                f,
                "the segment of external call {call} is empty, not led by the forwarder {program}"
            ),
            Self::NotLedByForwarder {
                call,
                program,
                first: Some(first),
            } => write!(
                f,
                "the segment of external call {call} starts with {first}, not the forwarder \
                 {program}"
            ),
        }
    }
}

impl std::error::Error for AccountsError {}

/// The forwarders registered with a submitter, by program.
#[derive(Default)]
pub struct Forwarders(HashMap<Pubkey, Arc<dyn Forwarder>>);

impl Forwarders {
    /// Registers `forwarder` as the one that supplies `program`'s calls.
    pub fn register(&mut self, program: Pubkey, forwarder: Arc<dyn Forwarder>) {
        self.0.insert(program, forwarder);
    }

    /// The accounts of `calls`: each call's CPI segment, in call order, and
    /// the instructions that precede the settlement, each forwarder's in the
    /// order of its first call. Each forwarder is asked once, for all its
    /// calls, and told where its preceding instructions start, after those
    /// of the forwarders before it.
    pub async fn accounts(
        &self,
        rpc: &RpcClient,
        calls: &[SolanaExternalCall],
    ) -> Result<CallAccounts, AccountsError> {
        // Each forwarder's calls by index, the forwarders in the order of
        // their first call.
        let mut by_forwarder: Vec<(Pubkey, Vec<usize>)> = Vec::new();
        for (i, call) in calls.iter().enumerate() {
            let program = Pubkey::new_from_array(call.program_id);
            match by_forwarder.iter_mut().find(|(p, _)| *p == program) {
                Some((_, indices)) => indices.push(i),
                None => by_forwarder.push((program, vec![i])),
            }
        }

        let mut preceding = Vec::new();
        let mut segments: Vec<Vec<AccountMeta>> = vec![Vec::new(); calls.len()];
        for (program, indices) in by_forwarder {
            let forwarder = self.0.get(&program).ok_or(AccountsError::Unregistered {
                call: indices[0],
                program,
            })?;
            let its_calls: Vec<SolanaExternalCall> =
                indices.iter().map(|&i| calls[i].clone()).collect();
            let accounts = forwarder
                .call_accounts(rpc, &its_calls, preceding.len())
                .await
                .map_err(|error| AccountsError::Forwarder {
                    calls: indices.clone(),
                    program,
                    error,
                })?;
            if accounts.segments.len() != indices.len() {
                return Err(AccountsError::SegmentCount {
                    program,
                    segments: accounts.segments.len(),
                    calls: indices.len(),
                });
            }
            for (&i, segment) in indices.iter().zip(accounts.segments) {
                if segment.len() != usize::from(calls[i].num_accounts) {
                    return Err(AccountsError::AccountCount {
                        call: i,
                        program,
                        accounts: segment.len(),
                        committed: calls[i].num_accounts,
                    });
                }
                let first = segment.first().map(|meta| meta.pubkey);
                if first != Some(program) {
                    return Err(AccountsError::NotLedByForwarder {
                        call: i,
                        program,
                        first,
                    });
                }
                segments[i] = segment;
            }
            preceding.extend(accounts.preceding);
        }
        Ok(CallAccounts {
            segments,
            preceding,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_call::OutputMode;
    use solana_instruction::Instruction;
    use std::sync::Mutex;

    /// A forwarder whose calls take the forwarder and `extra` accounts, and
    /// one preceding instruction per call naming its first instruction byte;
    /// it records the preceding start it is given each time it is asked.
    struct Fixed {
        extra: Vec<Pubkey>,
        starts: Mutex<Vec<usize>>,
    }

    impl Fixed {
        fn new(extra: Vec<Pubkey>) -> Arc<Self> {
            Arc::new(Fixed {
                extra,
                starts: Mutex::default(),
            })
        }

        fn starts(&self) -> Vec<usize> {
            self.starts.lock().unwrap().clone()
        }
    }

    impl Forwarder for Fixed {
        fn call_accounts<'a>(
            &'a self,
            _rpc: &'a RpcClient,
            calls: &'a [SolanaExternalCall],
            preceding_start: usize,
        ) -> CallAccountsFuture<'a> {
            Box::pin(async move {
                self.starts.lock().unwrap().push(preceding_start);
                let mut accounts = CallAccounts {
                    segments: vec![],
                    preceding: vec![],
                };
                for call in calls {
                    let program = Pubkey::new_from_array(call.program_id);
                    let mut segment = vec![AccountMeta::new_readonly(program, false)];
                    segment.extend(self.extra.iter().map(|k| AccountMeta::new(*k, false)));
                    accounts.segments.push(segment);
                    accounts.preceding.push(Instruction::new_with_bytes(
                        program,
                        &call.instruction_data[..1],
                        vec![],
                    ));
                }
                Ok(accounts)
            })
        }
    }

    /// A forwarder whose calls take no accounts at all.
    struct Empty;

    impl Forwarder for Empty {
        fn call_accounts<'a>(
            &'a self,
            _rpc: &'a RpcClient,
            calls: &'a [SolanaExternalCall],
            _preceding_start: usize,
        ) -> CallAccountsFuture<'a> {
            Box::pin(async move {
                Ok(CallAccounts {
                    segments: vec![vec![]; calls.len()],
                    preceding: vec![],
                })
            })
        }
    }

    fn call(program: Pubkey, byte: u8, num_accounts: u8) -> SolanaExternalCall {
        SolanaExternalCall {
            program_id: program.to_bytes(),
            instruction_data: vec![byte],
            expected_output: vec![1],
            output_mode: OutputMode::ReturnData,
            num_accounts,
        }
    }

    /// No forwarder here reads the chain, so no RPC is ever reached.
    fn rpc() -> RpcClient {
        RpcClient::new("http://127.0.0.1:1".to_string())
    }

    #[tokio::test]
    async fn each_forwarder_is_asked_once_for_all_its_calls() {
        let (a, b, x) = (
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        let (fixed_a, fixed_b) = (Fixed::new(vec![x]), Fixed::new(vec![]));
        let mut forwarders = Forwarders::default();
        forwarders.register(a, fixed_a.clone());
        forwarders.register(b, fixed_b.clone());

        let CallAccounts {
            preceding,
            segments,
        } = forwarders
            .accounts(&rpc(), &[call(a, 7, 2), call(b, 8, 1), call(a, 9, 2)])
            .await
            .unwrap();
        // Each forwarder is asked once, told where its preceding instructions
        // start: `a`'s two come first, so `b`'s start at 2.
        assert_eq!((fixed_a.starts(), fixed_b.starts()), (vec![0], vec![2]));
        // Each forwarder's preceding instructions, in the order of its first
        // call; the segments in call order.
        assert_eq!(
            preceding.iter().map(|ix| ix.data[0]).collect::<Vec<_>>(),
            [7, 9, 8]
        );
        assert_eq!(
            segments
                .iter()
                .map(|s| s.iter().map(|m| m.pubkey).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![vec![a, x], vec![b], vec![a, x]]
        );

        let unregistered = Pubkey::new_unique();
        assert_eq!(
            forwarders
                .accounts(&rpc(), &[call(a, 7, 2), call(unregistered, 1, 1)])
                .await
                .unwrap_err()
                .to_string(),
            format!("external call 1 is to {unregistered}, for which no forwarder is registered")
        );
        assert_eq!(
            forwarders
                .accounts(&rpc(), &[call(a, 7, 3)])
                .await
                .unwrap_err()
                .to_string(),
            format!("the forwarder {a} gives external call 0 2 accounts; the proof commits 3")
        );
    }

    /// A call whose proof commits no accounts, given an empty segment, is
    /// refused: a segment starts with the forwarder program.
    #[tokio::test]
    async fn an_empty_segment_is_refused() {
        let program = Pubkey::new_unique();
        let mut forwarders = Forwarders::default();
        forwarders.register(program, Arc::new(Empty));
        assert_eq!(
            forwarders
                .accounts(&rpc(), &[call(program, 7, 0)])
                .await
                .unwrap_err()
                .to_string(),
            format!("the segment of external call 0 is empty, not led by the forwarder {program}")
        );
    }
}

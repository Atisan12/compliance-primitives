//! Property-based fuzz harness for denylist-gate add/remove/check sequences.
//!
//! # Approach
//!
//! We use a lightweight in-process harness (plain `#[test]` + deterministic
//! PRNG) rather than `cargo-fuzz`/libFuzzer. A `#![no_std]` Soroban contract
//! crate cannot easily host a libFuzzer target in the same workspace without
//! pulling in `std`, and the invariant under test is simple enough that a
//! seeded loop gives strong coverage with fast CI-friendly defaults.
//!
//! # Invariant ("last write wins")
//!
//! For each address, after any sequence of `add_to_denylist`,
//! `remove_from_denylist`, and `check` calls:
//! - `check(addr)` returns `true` (clear) iff the most recent mutating call
//!   for `addr` was a remove, or `addr` was never added.
//! - `check(addr)` returns `false` (denied) iff the most recent mutating call
//!   for `addr` was an add.
//!
//! # Running
//!
//! Default (500 random sequences, suitable for `cargo test`):
//!
//! ```sh
//! cargo test -p denylist-gate fuzz_denylist_sequences
//! ```
//!
//! Longer local run: increase `DEFAULT_ITERATIONS` in this file (not wired into CI).

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};

const DEFAULT_ITERATIONS: u32 = 500;
const MAX_ADDRESSES: usize = 5;
const MAX_SEQUENCE_LEN: usize = 64;

#[derive(Clone, Copy)]
enum Op {
    Add,
    Remove,
    Check,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D)) >> 32) as u32
    }

    fn gen_usize(&mut self, upper: usize) -> usize {
        (self.next_u32() as usize) % upper
    }
}

fn run_sequence(seed: u64) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(DenylistGate, ());
    let client = DenylistGateClient::new(&env, &contract_id);
    client.initialize(&admin);

    let mut rng = Rng::new(seed);
    let mut addresses = Vec::new(&env);
    for _ in 0..MAX_ADDRESSES {
        addresses.push_back(Address::generate(&env));
    }
    let mut denied = [false; MAX_ADDRESSES];

    let seq_len = rng.gen_usize(MAX_SEQUENCE_LEN) + 1;
    for _ in 0..seq_len {
        let addr_idx = rng.gen_usize(MAX_ADDRESSES);
        let address = addresses.get(addr_idx as u32).unwrap();
        let op = match rng.gen_usize(3) {
            0 => Op::Add,
            1 => Op::Remove,
            _ => Op::Check,
        };

        match op {
            Op::Add => {
                client.add_to_denylist(&admin, &address);
                denied[addr_idx] = true;
            }
            Op::Remove => {
                client.remove_from_denylist(&admin, &address);
                denied[addr_idx] = false;
            }
            Op::Check => {
                let expected_clear = !denied[addr_idx];
                assert_eq!(
                    client.check(&address),
                    expected_clear,
                    "seed={seed} addr_idx={addr_idx}"
                );
            }
        }
    }

    for idx in 0..MAX_ADDRESSES {
        let address = addresses.get(idx as u32).unwrap();
        assert_eq!(
            client.check(&address),
            !denied[idx],
            "seed={seed} final addr_idx={idx}"
        );
    }
}

#[test]
fn fuzz_denylist_sequences() {
    for seed in 0..DEFAULT_ITERATIONS {
        run_sequence(seed as u64);
    }
}

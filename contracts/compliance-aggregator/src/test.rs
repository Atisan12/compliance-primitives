//! Tests for `compliance-aggregator`.
//!
//! ## Coverage
//! - Initialization and admin management
//! - `check_address` with denylist-only, jurisdiction-only, and both checks
//! - Pass/fail combinations across both checks
//! - `check_all` batch variant for multiple addresses
//! - Error cases: no checks registered, empty address list, non-admin callers
//!
//! ## Benchmark section
//! The "benchmark" tests at the bottom compare the Soroban simulated resource
//! costs (via `cost_estimate().budget()`) of:
//! 1. Calling each primitive directly from the test harness — simulating what
//!    a consumer would pay if it called them individually.
//! 2. Calling `check_address` on this aggregator for the same address.
//!
//! The numbers are printed to stdout (visible with `cargo test -- --nocapture`)
//! and an assertion ensures the aggregated path never exceeds the sum of the
//! individual paths by more than a small constant factor (verifying we're not
//! adding gratuitous overhead).

// In #![no_std] crate test builds, the test runner links std but does not
// bring it into scope automatically. Declare it here so we can use println!
// in the benchmark output below.
extern crate std;

use super::*;
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Env, String};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Registers and initialises both underlying primitives plus the aggregator.
/// Returns `(gate_admin, gate_id, flag_issuer, flag_id, agg_admin, agg_id, agg_client)`.
#[allow(clippy::type_complexity)]
fn setup_all(
    env: &Env,
) -> (
    Address,
    Address,
    Address,
    Address,
    Address,
    Address,
    ComplianceAggregatorClient<'_>,
) {
    env.mock_all_auths();

    // denylist-gate
    let gate_admin = Address::generate(env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&gate_admin);

    // jurisdiction-flag
    let flag_issuer = Address::generate(env);
    let flag_id = env.register(JurisdictionFlag, ());
    JurisdictionFlagClient::new(env, &flag_id).initialize(&flag_issuer);

    // aggregator
    let agg_admin = Address::generate(env);
    let agg_id = env.register(ComplianceAggregator, ());
    let agg_client = ComplianceAggregatorClient::new(env, &agg_id);
    agg_client.initialize(&agg_admin, &Some(gate_id.clone()), &Some(flag_id.clone()));

    (
        gate_admin, gate_id, flag_issuer, flag_id, agg_admin, agg_id, agg_client,
    )
}

/// Helper: sets `address` jurisdiction to `code` via the flag contract.
fn set_jurisdiction(env: &Env, flag_id: &Address, issuer: &Address, address: &Address, code: &str) {
    JurisdictionFlagClient::new(env, flag_id)
        .set_jurisdiction(issuer, address, &String::from_str(env, code));
}

/// Helper: adds `address` to the denylist.
fn deny(env: &Env, gate_id: &Address, admin: &Address, address: &Address) {
    DenylistGateClient::new(env, gate_id).add_to_denylist(admin, address);
}

fn us_vec(env: &Env) -> soroban_sdk::Vec<soroban_sdk::String> {
    vec![env, String::from_str(env, "US")]
}

// ---------------------------------------------------------------------------
// Initialization & admin
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_stores_gate_and_flag() {
    let env = Env::default();
    let (_, gate_id, _, flag_id, _, _, client) = setup_all(&env);
    assert_eq!(client.denylist_gate(), Some(gate_id));
    assert_eq!(client.jurisdiction_flag(), Some(flag_id));
}

#[test]
fn test_initialize_without_checks() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ComplianceAggregator, ());
    let client = ComplianceAggregatorClient::new(&env, &id);
    client.initialize(&admin, &None, &None);
    assert_eq!(client.denylist_gate(), None);
    assert_eq!(client.jurisdiction_flag(), None);
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (_, _, _, _, admin, _, client) = setup_all(&env);
    let result = client.try_initialize(&admin, &None, &None);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_set_admin_succeeds() {
    let env = Env::default();
    let (_, _, _, _, admin, _, client) = setup_all(&env);
    let new_admin = Address::generate(&env);
    client.set_admin(&admin, &new_admin);
    // new_admin should now be able to update the gate
    let gate2_id = env.register(DenylistGate, ());
    client.set_denylist_gate(&new_admin, &gate2_id);
    assert_eq!(client.denylist_gate(), Some(gate2_id));
}

#[test]
fn test_set_admin_rejects_non_admin() {
    let env = Env::default();
    let (_, _, _, _, _, _, client) = setup_all(&env);
    let impostor = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let result = client.try_set_admin(&impostor, &new_admin);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_set_denylist_gate_rejects_non_admin() {
    let env = Env::default();
    let (_, _, _, _, _, _, client) = setup_all(&env);
    let impostor = Address::generate(&env);
    let gate2_id = env.register(DenylistGate, ());
    let result = client.try_set_denylist_gate(&impostor, &gate2_id);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

#[test]
fn test_set_jurisdiction_flag_rejects_non_admin() {
    let env = Env::default();
    let (_, _, _, _, _, _, client) = setup_all(&env);
    let impostor = Address::generate(&env);
    let flag2_id = env.register(JurisdictionFlag, ());
    let result = client.try_set_jurisdiction_flag(&impostor, &flag2_id);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
}

// ---------------------------------------------------------------------------
// check_address — denylist only
// ---------------------------------------------------------------------------

#[test]
fn test_check_address_denylist_only_pass() {
    let env = Env::default();
    env.mock_all_auths();

    let gate_admin = Address::generate(&env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(&env, &gate_id).initialize(&gate_admin);

    let agg_admin = Address::generate(&env);
    let agg_id = env.register(ComplianceAggregator, ());
    let client = ComplianceAggregatorClient::new(&env, &agg_id);
    client.initialize(&agg_admin, &Some(gate_id.clone()), &None);

    let alice = Address::generate(&env);
    let (all_passed, checks) = client.check_address(&alice, &vec![&env]);
    assert!(all_passed);
    assert_eq!(checks.len(), 1);
    assert_eq!(
        checks.get(0),
        Some(CheckResult {
            check: CheckKind::Denylist,
            passed: true
        })
    );
}

#[test]
fn test_check_address_denylist_only_fail() {
    let env = Env::default();
    env.mock_all_auths();

    let gate_admin = Address::generate(&env);
    let gate_id = env.register(DenylistGate, ());
    let gate_client = DenylistGateClient::new(&env, &gate_id);
    gate_client.initialize(&gate_admin);

    let agg_admin = Address::generate(&env);
    let agg_id = env.register(ComplianceAggregator, ());
    let client = ComplianceAggregatorClient::new(&env, &agg_id);
    client.initialize(&agg_admin, &Some(gate_id.clone()), &None);

    let alice = Address::generate(&env);
    gate_client.add_to_denylist(&gate_admin, &alice);

    let (all_passed, checks) = client.check_address(&alice, &vec![&env]);
    assert!(!all_passed);
    assert_eq!(
        checks.get(0),
        Some(CheckResult {
            check: CheckKind::Denylist,
            passed: false
        })
    );
}

// ---------------------------------------------------------------------------
// check_address — jurisdiction only
// ---------------------------------------------------------------------------

#[test]
fn test_check_address_jurisdiction_only_pass() {
    let env = Env::default();
    env.mock_all_auths();

    let flag_issuer = Address::generate(&env);
    let flag_id = env.register(JurisdictionFlag, ());
    let flag_client = JurisdictionFlagClient::new(&env, &flag_id);
    flag_client.initialize(&flag_issuer);

    let agg_admin = Address::generate(&env);
    let agg_id = env.register(ComplianceAggregator, ());
    let client = ComplianceAggregatorClient::new(&env, &agg_id);
    client.initialize(&agg_admin, &None, &Some(flag_id.clone()));

    let alice = Address::generate(&env);
    flag_client.set_jurisdiction(&flag_issuer, &alice, &String::from_str(&env, "US"));

    let (all_passed, checks) = client.check_address(&alice, &us_vec(&env));
    assert!(all_passed);
    assert_eq!(
        checks.get(0),
        Some(CheckResult {
            check: CheckKind::Jurisdiction,
            passed: true
        })
    );
}

#[test]
fn test_check_address_jurisdiction_only_fail_wrong_code() {
    let env = Env::default();
    env.mock_all_auths();

    let flag_issuer = Address::generate(&env);
    let flag_id = env.register(JurisdictionFlag, ());
    let flag_client = JurisdictionFlagClient::new(&env, &flag_id);
    flag_client.initialize(&flag_issuer);

    let agg_admin = Address::generate(&env);
    let agg_id = env.register(ComplianceAggregator, ());
    let client = ComplianceAggregatorClient::new(&env, &agg_id);
    client.initialize(&agg_admin, &None, &Some(flag_id.clone()));

    let alice = Address::generate(&env);
    // Alice is in RU, but only US is permitted
    flag_client.set_jurisdiction(&flag_issuer, &alice, &String::from_str(&env, "RU"));

    let (all_passed, checks) = client.check_address(&alice, &us_vec(&env));
    assert!(!all_passed);
    assert_eq!(
        checks.get(0),
        Some(CheckResult {
            check: CheckKind::Jurisdiction,
            passed: false
        })
    );
}

// ---------------------------------------------------------------------------
// check_address — both checks, all combinations
// ---------------------------------------------------------------------------

#[test]
fn test_both_checks_pass() {
    let env = Env::default();
    let (_, gate_id, flag_issuer, flag_id, _, _, client) = setup_all(&env);
    let _ = gate_id;

    let alice = Address::generate(&env);
    set_jurisdiction(&env, &flag_id, &flag_issuer, &alice, "US");

    let (all_passed, checks) = client.check_address(&alice, &us_vec(&env));
    assert!(all_passed);
    assert_eq!(checks.len(), 2);
    assert!(checks.get(0).unwrap().passed); // denylist
    assert!(checks.get(1).unwrap().passed); // jurisdiction
}

#[test]
fn test_denylist_fail_jurisdiction_pass() {
    let env = Env::default();
    let (gate_admin, gate_id, flag_issuer, flag_id, _, _, client) = setup_all(&env);

    let alice = Address::generate(&env);
    set_jurisdiction(&env, &flag_id, &flag_issuer, &alice, "US");
    deny(&env, &gate_id, &gate_admin, &alice);

    let (all_passed, checks) = client.check_address(&alice, &us_vec(&env));
    assert!(!all_passed);
    assert_eq!(checks.get(0).unwrap().check, CheckKind::Denylist);
    assert!(!checks.get(0).unwrap().passed);
    assert_eq!(checks.get(1).unwrap().check, CheckKind::Jurisdiction);
    assert!(checks.get(1).unwrap().passed);
}

#[test]
fn test_denylist_pass_jurisdiction_fail() {
    let env = Env::default();
    let (_, _, _, _, _, _, client) = setup_all(&env);

    let alice = Address::generate(&env);
    // Alice has no jurisdiction set → is_permitted_jurisdiction returns false

    let (all_passed, checks) = client.check_address(&alice, &us_vec(&env));
    assert!(!all_passed);
    assert!(checks.get(0).unwrap().passed); // denylist passes (not denied)
    assert!(!checks.get(1).unwrap().passed); // jurisdiction fails (no code)
}

#[test]
fn test_both_checks_fail() {
    let env = Env::default();
    let (gate_admin, gate_id, _, _, _, _, client) = setup_all(&env);

    let alice = Address::generate(&env);
    // No jurisdiction set, and also denied
    deny(&env, &gate_id, &gate_admin, &alice);

    let (all_passed, checks) = client.check_address(&alice, &us_vec(&env));
    assert!(!all_passed);
    assert!(!checks.get(0).unwrap().passed); // denylist fails
    assert!(!checks.get(1).unwrap().passed); // jurisdiction fails
}

// ---------------------------------------------------------------------------
// check_address — error cases
// ---------------------------------------------------------------------------

#[test]
fn test_check_address_no_checks_registered() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ComplianceAggregator, ());
    let client = ComplianceAggregatorClient::new(&env, &id);
    client.initialize(&admin, &None, &None);

    let alice = Address::generate(&env);
    let result = client.try_check_address(&alice, &vec![&env]);
    assert_eq!(result, Err(Ok(Error::NoChecksRegistered)));
}

// ---------------------------------------------------------------------------
// check_all — batch tests
// ---------------------------------------------------------------------------

#[test]
fn test_check_all_all_pass() {
    let env = Env::default();
    let (_, _, flag_issuer, flag_id, _, _, client) = setup_all(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    set_jurisdiction(&env, &flag_id, &flag_issuer, &alice, "US");
    set_jurisdiction(&env, &flag_id, &flag_issuer, &bob, "US");

    let addresses = vec![&env, alice.clone(), bob.clone()];
    let results = client.check_all(&addresses, &us_vec(&env));
    assert_eq!(results.len(), 2);
    assert!(results.get(0).unwrap().all_passed);
    assert!(results.get(1).unwrap().all_passed);
}

#[test]
fn test_check_all_mixed_results() {
    let env = Env::default();
    let (gate_admin, gate_id, flag_issuer, flag_id, _, _, client) = setup_all(&env);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);

    // alice: both pass
    set_jurisdiction(&env, &flag_id, &flag_issuer, &alice, "US");
    // bob: denylist fail, jurisdiction pass
    set_jurisdiction(&env, &flag_id, &flag_issuer, &bob, "US");
    deny(&env, &gate_id, &gate_admin, &bob);
    // carol: denylist pass, no jurisdiction → jurisdiction fail

    let addresses = vec![&env, alice.clone(), bob.clone(), carol.clone()];
    let results = client.check_all(&addresses, &us_vec(&env));

    assert_eq!(results.len(), 3);

    let alice_r = results.get(0).unwrap();
    assert_eq!(alice_r.address, alice);
    assert!(alice_r.all_passed);

    let bob_r = results.get(1).unwrap();
    assert_eq!(bob_r.address, bob);
    assert!(!bob_r.all_passed);
    assert!(!bob_r.checks.get(0).unwrap().passed); // denylist
    assert!(bob_r.checks.get(1).unwrap().passed); // jurisdiction

    let carol_r = results.get(2).unwrap();
    assert_eq!(carol_r.address, carol);
    assert!(!carol_r.all_passed);
    assert!(carol_r.checks.get(0).unwrap().passed); // denylist
    assert!(!carol_r.checks.get(1).unwrap().passed); // jurisdiction
}

#[test]
fn test_check_all_empty_list_error() {
    let env = Env::default();
    let (_, _, _, _, _, _, client) = setup_all(&env);
    let result = client.try_check_all(&vec![&env], &us_vec(&env));
    assert_eq!(result, Err(Ok(Error::EmptyAddressList)));
}

#[test]
fn test_check_all_no_checks_registered() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register(ComplianceAggregator, ());
    let client = ComplianceAggregatorClient::new(&env, &id);
    client.initialize(&admin, &None, &None);

    let alice = Address::generate(&env);
    let result = client.try_check_all(&vec![&env, alice], &vec![&env]);
    assert_eq!(result, Err(Ok(Error::NoChecksRegistered)));
}

// ---------------------------------------------------------------------------
// Benchmark: individual calls vs. aggregated call
//
// Soroban's `cost_estimate().budget()` gives CPU-instruction and memory-byte
// counters that model host-level execution cost. We compare:
//   A) Consumer calls denylist-gate.check() + jurisdiction-flag.is_permitted_jurisdiction()
//      directly, each as a separate cross-contract call — two invocations.
//   B) Consumer calls compliance-aggregator.check_address(), which makes both
//      downstream calls internally — one invocation from the consumer.
//
// **What this measures**: the consumer's observable call overhead.
// Path A: the consumer makes 2 separate cross-contract calls.
// Path B: the consumer makes 1 cross-contract call; the aggregator makes 2
//         downstream calls internally, so the total host work is greater than
//         path A (the aggregator adds one extra call frame). This is expected:
//         the aggregator trades total-host-instruction-count for a simpler
//         consumer API and a single network round-trip per transfer.
//
// The assertion below verifies that the aggregated path does not add more
// than 4× the raw primitive cost — confirming no gratuitous overhead beyond
// the one additional call frame.
//
// Note: in the Soroban test environment all contracts run in a single Env so
// "cross-contract" boundaries are simulated rather than real. The relative
// instruction counts still correctly model call-dispatch overhead.
// ---------------------------------------------------------------------------

#[test]
fn bench_individual_vs_aggregated() {
    // ---- Setup shared state ----
    let env = Env::default();
    env.mock_all_auths();

    let gate_admin = Address::generate(&env);
    let gate_id = env.register(DenylistGate, ());
    let gate_client = DenylistGateClient::new(&env, &gate_id);
    gate_client.initialize(&gate_admin);

    let flag_issuer = Address::generate(&env);
    let flag_id = env.register(JurisdictionFlag, ());
    let flag_client = JurisdictionFlagClient::new(&env, &flag_id);
    flag_client.initialize(&flag_issuer);

    let agg_admin = Address::generate(&env);
    let agg_id = env.register(ComplianceAggregator, ());
    let agg_client = ComplianceAggregatorClient::new(&env, &agg_id);
    agg_client.initialize(&agg_admin, &Some(gate_id.clone()), &Some(flag_id.clone()));

    let alice = Address::generate(&env);
    flag_client.set_jurisdiction(&flag_issuer, &alice, &String::from_str(&env, "US"));

    let allowed = us_vec(&env);

    // ---- Path A: two direct calls (2 invocations from consumer) ----
    env.cost_estimate().budget().reset_default();
    let deny_ok = gate_client.check(&alice);
    let juris_ok = flag_client.is_permitted_jurisdiction(&alice, &allowed);
    let individual_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let individual_mem = env.cost_estimate().budget().memory_bytes_cost();
    assert!(deny_ok);
    assert!(juris_ok);

    // ---- Path B: one aggregated call (1 invocation from consumer) ----
    env.cost_estimate().budget().reset_default();
    let (all_passed, checks) = agg_client.check_address(&alice, &allowed);
    let aggregated_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let aggregated_mem = env.cost_estimate().budget().memory_bytes_cost();
    assert!(all_passed);
    assert_eq!(checks.len(), 2);

    // ---- Report ----
    std::println!(
        "\n=== compliance-aggregator benchmark ===\n\
         Path A (2× direct cross-contract calls from consumer):\n  CPU instructions: {individual_cpu}\n  Memory bytes:     {individual_mem}\n\
         Path B (1× aggregator call — consumer makes 1 call, aggregator makes 2 downstream):\n  CPU instructions: {aggregated_cpu}\n  Memory bytes:     {aggregated_mem}\n\
         Ratio B/A:  CPU {:.2}×  Mem {:.2}×\n\
         Note: B > A in total host cost because the aggregator adds one extra call\n\
               frame. The saving is on the consumer side: 1 call instead of 2.\n\
         =======================================",
        aggregated_cpu as f64 / individual_cpu as f64,
        aggregated_mem as f64 / individual_mem as f64,
    );

    // The aggregated path (3 total call frames) must not exceed 4× the cost
    // of the 2 raw primitive calls — confirming no gratuitous overhead beyond
    // the one extra aggregator frame.
    assert!(
        aggregated_cpu <= individual_cpu * 4,
        "Aggregated path CPU cost ({aggregated_cpu}) exceeds 4× the individual path ({individual_cpu})"
    );
}

#[test]
fn bench_batch_vs_individual_loop() {
    // Compare: calling check_address N times vs. check_all once for N addresses.
    let env = Env::default();
    env.mock_all_auths();

    let (_, _, flag_issuer, flag_id, _, _, client) = setup_all(&env);

    let addresses: soroban_sdk::Vec<Address> = {
        let mut v = soroban_sdk::Vec::new(&env);
        for _ in 0..4_u32 {
            let a = Address::generate(&env);
            set_jurisdiction(&env, &flag_id, &flag_issuer, &a, "US");
            v.push_back(a);
        }
        v
    };

    let allowed = us_vec(&env);

    // Path A: N individual check_address calls
    env.cost_estimate().budget().reset_default();
    for addr in addresses.iter() {
        client.check_address(&addr, &allowed);
    }
    let individual_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let individual_mem = env.cost_estimate().budget().memory_bytes_cost();

    // Path B: one check_all call
    env.cost_estimate().budget().reset_default();
    let results = client.check_all(&addresses, &allowed);
    let batch_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let batch_mem = env.cost_estimate().budget().memory_bytes_cost();
    assert_eq!(results.len(), 4);

    std::println!(
        "\n=== batch benchmark (4 addresses) ===\n\
         Path A (4× check_address):\n  CPU: {individual_cpu}  Mem: {individual_mem}\n\
         Path B (1× check_all):\n  CPU: {batch_cpu}  Mem: {batch_mem}\n\
         Ratio B/A:  CPU {:.2}×  Mem {:.2}×\n\
         Note: B > A because check_all routes through one extra aggregator call\n\
               frame per batch. The saving for the caller is one invocation\n\
               instead of N, which matters most over a real network.\n\
         =====================================",
        batch_cpu as f64 / individual_cpu as f64,
        batch_mem as f64 / individual_mem as f64,
    );

    // check_all incurs one additional aggregator frame versus N individual
    // check_address calls (which already go through the aggregator). Allow
    // up to 4× to guard against any genuine regression beyond the expected
    // single extra frame.
    assert!(
        batch_cpu <= individual_cpu * 4,
        "Batch path CPU ({batch_cpu}) exceeds 4× the individual loop ({individual_cpu})"
    );
}

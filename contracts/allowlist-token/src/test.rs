use super::*;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{contract, contractimpl, symbol_short, vec, Env, IntoVal, Map, Symbol, Val};
use std::path::{Path, PathBuf};

/// A minimal token double used only by these tests, so `allowlist-token`'s
/// unit tests don't depend on any particular real SEP-41 implementation.
#[contract]
struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        env.storage().instance().set(&Symbol::new(&env, "last"), &(from, to, amount));
    }

    pub fn last_transfer(env: Env) -> Option<(Address, Address, i128)> {
        env.storage().instance().get(&Symbol::new(&env, "last"))
    }
}

fn setup(env: &Env) -> (Address, Address, Address, AllowlistTokenClient<'_>) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let token_id = env.register(MockToken, ());
    let contract_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(env, &contract_id);
    client.initialize(&admin, &token_id);
    (admin, token_id, contract_id, client)
}

fn read_baseline(path: &Path, section: &str) -> (u64, u64) {
    let contents = std::fs::read_to_string(path).unwrap();
    let section_header = format!("[{section}]");
    let mut in_section = false;
    let mut cpu = None;
    let mut memory = None;

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == section_header;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("cpu = ") {
            cpu = Some(value.parse::<u64>().unwrap());
        } else if let Some(value) = trimmed.strip_prefix("memory = ") {
            memory = Some(value.parse::<u64>().unwrap());
        }
    }

    let cpu = cpu.expect("missing cpu baseline");
    let memory = memory.expect("missing memory baseline");
    (cpu, memory)
}

fn baseline_path_for_manifest_dir(manifest_dir: PathBuf) -> PathBuf {
    manifest_dir.join("..").join("..").join("budget-baselines.toml")
}

fn assert_budget_within_threshold(measured: (u64, u64), baseline: (u64, u64), label: &str) {
    let (measured_cpu, measured_memory) = measured;
    let (baseline_cpu, baseline_memory) = baseline;
    let cpu_limit = (baseline_cpu as f64 * 1.10).ceil() as u64;
    let memory_limit = (baseline_memory as f64 * 1.10).ceil() as u64;

    assert!(
        measured_cpu <= cpu_limit,
        "{label} CPU regression: measured {measured_cpu}, baseline {baseline_cpu}, limit {cpu_limit}"
    );
    assert!(
        measured_memory <= memory_limit,
        "{label} memory regression: measured {measured_memory}, baseline {baseline_memory}, limit {memory_limit}"
    );
}

#[test]
fn test_initialize_and_allowlist_roundtrip() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    assert!(!client.is_allowed(&alice));
    client.add_to_allowlist(&admin, &alice);
    assert!(client.is_allowed(&alice));
    client.remove_from_allowlist(&admin, &alice);
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_transfer_forwards_to_underlying_token_when_both_allowlisted() {
    let env = Env::default();
    let (admin, token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);
    client.add_to_allowlist(&admin, &bob);

    let ok = client.transfer(&alice, &bob, &500);
    assert!(ok);

    let token_client = MockTokenClient::new(&env, &token_id);
    let last = token_client.last_transfer().unwrap();
    assert_eq!(last, (alice, bob, 500));
}

#[test]
fn test_budget_regression_allowlist_transfer() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);
    client.add_to_allowlist(&admin, &bob);

    let mut budget = env.cost_estimate().budget();
    budget.reset_default();
    let ok = client.transfer(&alice, &bob, &500);
    assert!(ok);

    let measured = (budget.cpu_instruction_cost(), budget.memory_bytes_cost());
    let baseline_path = baseline_path_for_manifest_dir(PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()));
    let baseline = read_baseline(&baseline_path, "allowlist-token.transfer");
    assert_budget_within_threshold(measured, baseline, "allowlist-token transfer");
}

#[test]
fn test_transfer_blocked_when_recipient_not_allowlisted() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);

    let ok = client.transfer(&alice, &bob, &500);
    assert!(!ok);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_add"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
            (
                contract_id.clone(),
                (symbol_short!("blocked"), alice.clone(), bob.clone()).into_val(&env),
                Map::<Symbol, Val>::from_array(&env, [(symbol_short!("amount"), 500i128.into_val(&env))])
                    .into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_add_to_allowlist_rejects_non_admin() {
    let env = Env::default();
    let (_admin, _token_id, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    let result = client.try_add_to_allowlist(&impostor, &alice);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_non_admin_allowlist_mutations_rejected_end_to_end() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    let add_result = client.try_add_to_allowlist(&impostor, &alice);
    assert_eq!(add_result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_allowed(&alice));

    client.add_to_allowlist(&admin, &alice);
    assert!(client.is_allowed(&alice));

    let remove_result = client.try_remove_from_allowlist(&impostor, &alice);
    assert_eq!(remove_result, Err(Ok(Error::NotAuthorized)));
    assert!(client.is_allowed(&alice));
}

#[test]
fn test_remove_from_allowlist_never_added_is_noop() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let never_added = Address::generate(&env);

    assert!(!client.is_allowed(&never_added));

    client.remove_from_allowlist(&admin, &never_added);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_remove"), never_added.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
    assert!(!client.is_allowed(&never_added));
}

#[test]
fn test_is_allowed_false_before_initialize() {
    let env = Env::default();
    let contract_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(&env, &contract_id);
    let alice = Address::generate(&env);

    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_get_admin_returns_initialized_admin() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);

    assert_eq!(client.get_admin(), admin);
}

#[test]
fn test_get_admin_fails_before_initialize() {
    let env = Env::default();
    let contract_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(&env, &contract_id);

    let result = client.try_get_admin();
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (admin, token_id, _contract_id, client) = setup(&env);
    let result = client.try_initialize(&admin, &token_id);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_add_to_allowlist_emits_allow_add_event() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_add"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_remove_from_allowlist_emits_allow_remove_event() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    client.add_to_allowlist(&admin, &alice);

    client.remove_from_allowlist(&admin, &alice);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_add"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_remove"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_admin_transfer_propose_and_accept_flow() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let new_admin = Address::generate(&env);
    let alice = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);

    // Existing admin remains effective until the pending admin accepts.
    client.add_to_allowlist(&admin, &alice);
    let pre_accept_remove = client.try_remove_from_allowlist(&new_admin, &alice);
    assert_eq!(pre_accept_remove, Err(Ok(Error::NotAuthorized)));
    assert!(client.is_allowed(&alice));

    client.accept_admin(&new_admin);

    let old_admin_remove = client.try_remove_from_allowlist(&admin, &alice);
    assert_eq!(old_admin_remove, Err(Ok(Error::NotAuthorized)));

    client.remove_from_allowlist(&new_admin, &alice);
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_accept_admin_rejects_wrong_address() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let proposed = Address::generate(&env);
    let wrong = Address::generate(&env);

    client.propose_admin(&admin, &proposed);

    let result = client.try_accept_admin(&wrong);
    assert_eq!(result, Err(Ok(Error::PendingAdminMismatch)));
}

#[test]
fn test_propose_admin_rejects_non_admin() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let alice = Address::generate(&env);

    let result = client.try_propose_admin(&impostor, &new_admin);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));

    client.add_to_allowlist(&admin, &alice);
    assert!(client.is_allowed(&alice));
}

#[test]
fn test_accept_admin_emits_admin_transferred_event() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let new_admin = Address::generate(&env);

    client.propose_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "admin_transferred"), admin.clone(), new_admin.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
}

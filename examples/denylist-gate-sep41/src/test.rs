use super::*;
use denylist_gate::{DenylistGate, DenylistGateClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

// ---------------------------------------------------------------------------
// Test setup helper
// ---------------------------------------------------------------------------

fn setup(env: &Env) -> (Address, Address, Address, Sep41GatedTokenClient<'_>) {
    env.mock_all_auths();

    // Deploy and initialize a fresh denylist-gate instance.
    let gate_admin = Address::generate(env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&gate_admin);

    // Deploy and initialize the SEP-41 gated token.
    let token_admin = Address::generate(env);
    let token_id = env.register(Sep41GatedToken, ());
    let client = Sep41GatedTokenClient::new(env, &token_id);
    client.initialize(
        &token_admin,
        &gate_id,
        &7u32,
        &String::from_str(env, "Gated Token"),
        &String::from_str(env, "GTOK"),
    );

    (gate_admin, gate_id, token_admin, client)
}

// ---------------------------------------------------------------------------
// Metadata tests
// ---------------------------------------------------------------------------

#[test]
fn test_sep41_metadata() {
    let env = Env::default();
    let (_gate_admin, _gate_id, _token_admin, client) = setup(&env);
    assert_eq!(client.decimals(), 7);
    assert_eq!(client.name(), String::from_str(&env, "Gated Token"));
    assert_eq!(client.symbol(), String::from_str(&env, "GTOK"));
}

// ---------------------------------------------------------------------------
// Transfer — success case (both parties clear of denylist)
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_succeeds_when_both_clear() {
    let env = Env::default();
    let (_gate_admin, _gate_id, token_admin, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&token_admin, &alice, &1_000);
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);

    client.transfer(&alice, &bob, &400);

    assert_eq!(client.balance(&alice), 600);
    assert_eq!(client.balance(&bob), 400);
}

// ---------------------------------------------------------------------------
// Transfer — denied cases
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_denied_when_sender_on_denylist() {
    let env = Env::default();
    let (gate_admin, gate_id, token_admin, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&token_admin, &alice, &1_000);
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &alice);

    let result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    // Balances must be unchanged — the transfer was reverted.
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);
}

#[test]
fn test_transfer_denied_when_recipient_on_denylist() {
    let env = Env::default();
    let (gate_admin, gate_id, token_admin, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&token_admin, &alice, &1_000);
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &bob);

    let result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);
}

// ---------------------------------------------------------------------------
// transfer_from — success and denied
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_from_succeeds_when_both_clear() {
    let env = Env::default();
    let (_gate_admin, _gate_id, token_admin, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let spender = Address::generate(&env);

    client.mint(&token_admin, &alice, &1_000);
    client.approve(&alice, &spender, &500, &9999u32);
    client.transfer_from(&spender, &alice, &bob, &300);

    assert_eq!(client.balance(&alice), 700);
    assert_eq!(client.balance(&bob), 300);
    assert_eq!(client.allowance(&alice, &spender), 200);
}

#[test]
fn test_transfer_from_denied_when_sender_on_denylist() {
    let env = Env::default();
    let (gate_admin, gate_id, token_admin, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let spender = Address::generate(&env);

    client.mint(&token_admin, &alice, &1_000);
    client.approve(&alice, &spender, &500, &9999u32);
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &alice);

    let result = client.try_transfer_from(&spender, &alice, &bob, &300);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    assert_eq!(client.balance(&alice), 1_000);
}

// ---------------------------------------------------------------------------
// Allowance / burn coverage
// ---------------------------------------------------------------------------

#[test]
fn test_burn_reduces_balance() {
    let env = Env::default();
    let (_gate_admin, _gate_id, token_admin, client) = setup(&env);
    let alice = Address::generate(&env);
    client.mint(&token_admin, &alice, &500);
    client.burn(&alice, &200);
    assert_eq!(client.balance(&alice), 300);
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (_gate_admin, gate_id, token_admin, client) = setup(&env);
    let result = client.try_initialize(
        &token_admin,
        &gate_id,
        &7u32,
        &String::from_str(&env, "X"),
        &String::from_str(&env, "X"),
    );
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

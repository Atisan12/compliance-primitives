use super::*;
use circuit_breaker::{CircuitBreaker, CircuitBreakerClient};
use denylist_gate::{DenylistGate, DenylistGateClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

fn setup(env: &Env) -> (Address, Address, Address, Address, ExampleTokenClient<'_>) {
    env.mock_all_auths();
    let gate_admin = Address::generate(env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&gate_admin);

    let breaker_admin = Address::generate(env);
    let breaker_id = env.register(CircuitBreaker, ());
    CircuitBreakerClient::new(env, &breaker_id).initialize(&breaker_admin);

    let token_id = env.register(ExampleToken, ());
    let client = ExampleTokenClient::new(env, &token_id);
    client.initialize(&gate_id, &breaker_id);
    (gate_admin, gate_id, breaker_admin, breaker_id, client)
}

#[test]
fn test_transfer_succeeds_when_both_parties_clear() {
    let env = Env::default();
    let (_gate_admin, _gate_id, _breaker_admin, _breaker_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&alice, &1_000);
    client.transfer(&alice, &bob, &400);

    assert_eq!(client.balance(&alice), 600);
    assert_eq!(client.balance(&bob), 400);
}

#[test]
fn test_transfer_blocked_when_sender_denied() {
    let env = Env::default();
    let (gate_admin, gate_id, _breaker_admin, _breaker_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&alice, &1_000);
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &alice);

    let result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);
}

#[test]
fn test_gate_is_rechecked_on_every_transfer_not_cached() {
    let env = Env::default();
    let (gate_admin, gate_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&alice, &1_000);

    // Initial transfer succeeds while both parties are clear.
    client.transfer(&alice, &bob, &400);
    assert_eq!(client.balance(&alice), 600);
    assert_eq!(client.balance(&bob), 400);

    // Deny one of the parties mid-flow, after the successful transfer.
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &bob);

    // A subsequent transfer touching the now-denied party must be blocked,
    // proving the gate is re-checked on every call rather than cached from
    // the earlier successful check.
    let result = client.try_transfer(&alice, &bob, &100);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    assert_eq!(client.balance(&alice), 600);
    assert_eq!(client.balance(&bob), 400);

    let result = client.try_transfer(&bob, &alice, &100);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    assert_eq!(client.balance(&alice), 600);
    assert_eq!(client.balance(&bob), 400);
}

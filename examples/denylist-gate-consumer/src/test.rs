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
fn test_transfer_blocked_when_breaker_frozen() {
    let env = Env::default();
    let (_gate_admin, _gate_id, breaker_admin, breaker_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&alice, &1_000);
    CircuitBreakerClient::new(&env, &breaker_id).freeze(&breaker_admin);

    let result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::FrozenByBreaker)));
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);
}

#[test]
fn test_transfer_resumes_after_breaker_unfreeze() {
    let env = Env::default();
    let (_gate_admin, _gate_id, breaker_admin, breaker_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&alice, &1_000);
    CircuitBreakerClient::new(&env, &breaker_id).freeze(&breaker_admin);

    let frozen_result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(frozen_result, Err(Ok(Error::FrozenByBreaker)));

    CircuitBreakerClient::new(&env, &breaker_id).unfreeze(&breaker_admin);
    let resumed = client.transfer(&alice, &bob, &400);
    assert!(resumed.is_ok());
    assert_eq!(client.balance(&alice), 600);
    assert_eq!(client.balance(&bob), 400);
}

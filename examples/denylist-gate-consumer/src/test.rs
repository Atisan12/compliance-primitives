use super::*;
use denylist_gate::{DenylistGate, DenylistGateClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

fn setup(env: &Env) -> (Address, Address, ExampleTokenClient<'_>) {
    env.mock_all_auths();
    let gate_admin = Address::generate(env);
    let gate_id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &gate_id).initialize(&gate_admin);

    let token_id = env.register(ExampleToken, ());
    let client = ExampleTokenClient::new(env, &token_id);
    client.initialize(&gate_id);
    (gate_admin, gate_id, client)
}

#[test]
fn test_transfer_succeeds_when_both_parties_clear() {
    let env = Env::default();
    let (_gate_admin, _gate_id, client) = setup(&env);
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
    let (gate_admin, gate_id, client) = setup(&env);
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
fn test_transfer_blocked_when_recipient_denied() {
    let env = Env::default();
    let (gate_admin, gate_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&alice, &1_000);
    DenylistGateClient::new(&env, &gate_id).add_to_denylist(&gate_admin, &bob);

    let result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::DeniedByGate)));
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);
}

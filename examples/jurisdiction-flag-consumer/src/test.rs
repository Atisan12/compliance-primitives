use super::*;
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Env, String};

fn setup(env: &Env) -> (Address, Address, ExampleTokenClient<'_>) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let flag_id = env.register(JurisdictionFlag, ());
    JurisdictionFlagClient::new(env, &flag_id).initialize(&issuer);

    let allowed = vec![
        env,
        String::from_str(env, "US"),
        String::from_str(env, "CA"),
    ];

    let token_id = env.register(ExampleToken, ());
    let client = ExampleTokenClient::new(env, &token_id);
    client.initialize(&flag_id, &allowed);
    (issuer, flag_id, client)
}

#[test]
fn test_transfer_succeeds_for_permitted_jurisdiction() {
    let env = Env::default();
    let (issuer, flag_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    JurisdictionFlagClient::new(&env, &flag_id).set_jurisdiction(
        &issuer,
        &alice,
        &String::from_str(&env, "US"),
    );

    client.mint(&alice, &1_000);
    client.transfer(&alice, &bob, &400);

    assert_eq!(client.balance(&alice), 600);
    assert_eq!(client.balance(&bob), 400);
}

#[test]
fn test_transfer_rejected_for_non_permitted_jurisdiction() {
    let env = Env::default();
    let (issuer, flag_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    JurisdictionFlagClient::new(&env, &flag_id).set_jurisdiction(
        &issuer,
        &alice,
        &String::from_str(&env, "GB"),
    );

    client.mint(&alice, &1_000);

    let result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::JurisdictionNotPermitted)));
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);
}

#[test]
fn test_transfer_rejected_when_no_jurisdiction_set() {
    let env = Env::default();
    let (_issuer, _flag_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.mint(&alice, &1_000);

    let result = client.try_transfer(&alice, &bob, &400);
    assert_eq!(result, Err(Ok(Error::JurisdictionNotPermitted)));
    assert_eq!(client.balance(&alice), 1_000);
    assert_eq!(client.balance(&bob), 0);
}

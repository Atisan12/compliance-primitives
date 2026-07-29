use super::*;
use soroban_sdk::testutils::{storage::Persistent as _, Address as _, Events as _, Ledger as _};
use soroban_sdk::{vec, Env};

fn setup(env: &Env) -> (Address, Address, JurisdictionFlagClient<'_>) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let contract_id = env.register(JurisdictionFlag, ());
    let client = JurisdictionFlagClient::new(env, &contract_id);
    client.initialize(&issuer);
    (issuer, contract_id, client)
}

#[test]
fn test_set_and_get_jurisdiction() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    assert_eq!(client.get_jurisdiction(&alice), None);

    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);
    assert_eq!(client.get_jurisdiction(&alice), Some(code));
}

#[test]
fn test_set_jurisdiction_rejects_non_issuer() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    let result = client.try_set_jurisdiction(&impostor, &alice, &code);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert_eq!(client.get_jurisdiction(&alice), None);
}

#[test]
fn test_is_permitted_jurisdiction_true_when_code_in_list() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);

    let allowed = vec![
        &env,
        String::from_str(&env, "CA"),
        String::from_str(&env, "US"),
    ];
    assert!(client.is_permitted_jurisdiction(&alice, &allowed));
}

#[test]
fn test_is_permitted_jurisdiction_false_when_no_jurisdiction_set() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let allowed = vec![&env, String::from_str(&env, "US")];
    assert!(!client.is_permitted_jurisdiction(&alice, &allowed));
}

#[test]
fn test_is_permitted_jurisdiction_false_with_empty_allowed_list() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);

    let allowed: Vec<String> = vec![&env];
    assert!(!client.is_permitted_jurisdiction(&alice, &allowed));
}

#[test]
fn test_is_permitted_jurisdiction_false_when_no_jurisdiction_and_empty_allowed_list() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    let allowed: Vec<String> = vec![&env];
    assert!(!client.is_permitted_jurisdiction(&alice, &allowed));
}

#[test]
fn test_set_jurisdiction_fails_before_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(JurisdictionFlag, ());
    let client = JurisdictionFlagClient::new(&env, &contract_id);
    let issuer = Address::generate(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    let result = client.try_set_jurisdiction(&issuer, &alice, &code);
    assert_eq!(result, Err(Ok(Error::NotInitialized)));
    assert_eq!(env.events().all(), vec![&env]);
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let result = client.try_initialize(&issuer);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_set_jurisdiction_extends_persistent_ttl() {
    let env = Env::default();
    let (issuer, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    client.set_jurisdiction(&issuer, &alice, &code);

    let key = DataKey::Jurisdiction(alice.clone());

    // Advance the ledger until the entry TTL drops below the extension threshold.
    env.ledger().with_mut(|li| {
        li.sequence_number += super::TTL_EXTEND_TO - super::TTL_THRESHOLD + 1;
    });

    let ttl_before_read = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert!(ttl_before_read < super::TTL_THRESHOLD);

    assert_eq!(client.get_jurisdiction(&alice), Some(code));

    let ttl_after_read = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert_eq!(ttl_after_read, super::TTL_EXTEND_TO);

    env.ledger().with_mut(|li| {
        li.sequence_number += super::TTL_EXTEND_TO - super::TTL_THRESHOLD + 1;
    });

    let ttl_before_rewrite = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert!(ttl_before_rewrite < super::TTL_THRESHOLD);

    let updated = String::from_str(&env, "CA");
    client.set_jurisdiction(&issuer, &alice, &updated);

    let ttl_after_write = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&key)
    });
    assert_eq!(ttl_after_write, super::TTL_EXTEND_TO);
}

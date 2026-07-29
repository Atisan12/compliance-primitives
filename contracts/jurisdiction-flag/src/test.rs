use super::*;
use soroban_sdk::testutils::{Address as _, Events as _};
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

// ── compliance-officer tests ───────────────────────────────────────────

#[test]
fn test_issuer_can_set_and_revoke_compliance_officer() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);

    // Set compliance officer
    client.set_compliance_officer(&issuer, &officer);

    // Officer can set jurisdiction
    let alice = Address::generate(&env);
    let code_us = String::from_str(&env, "US");
    client.set_jurisdiction(&officer, &alice, &code_us);
    assert_eq!(client.get_jurisdiction(&alice), Some(code_us));

    // Revoke compliance officer
    client.revoke_compliance_officer(&issuer);

    // Now officer cannot set another jurisdiction
    let bob = Address::generate(&env);
    let result = client.try_set_jurisdiction(&officer, &bob, &code_us);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert_eq!(client.get_jurisdiction(&bob), None);
}

#[test]
fn test_compliance_officer_can_set_jurisdiction() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&issuer, &officer);

    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    // Officer can set jurisdiction
    client.set_jurisdiction(&officer, &alice, &code);
    assert_eq!(client.get_jurisdiction(&alice), Some(code));
}

#[test]
fn test_compliance_officer_cannot_set_or_revoke_role() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&issuer, &officer);

    let another = Address::generate(&env);

    // Officer cannot set another compliance officer
    let result = client.try_set_compliance_officer(&officer, &another);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));

    // Officer cannot revoke own role
    let result = client.try_revoke_compliance_officer(&officer);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));

    // Officer still has their role
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&officer, &alice, &code);
    assert_eq!(client.get_jurisdiction(&alice), Some(code));
}

#[test]
fn test_issuer_can_still_perform_compliance_actions() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&issuer, &officer);

    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    // Issuer can still set jurisdiction directly
    client.set_jurisdiction(&issuer, &alice, &code);
    assert_eq!(client.get_jurisdiction(&alice), Some(code));
}

#[test]
fn test_unset_officer_rejected_for_compliance_actions() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);

    // No compliance officer set — unknown address cannot act
    let rando = Address::generate(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    let result = client.try_set_jurisdiction(&rando, &alice, &code);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert_eq!(client.get_jurisdiction(&alice), None);
}

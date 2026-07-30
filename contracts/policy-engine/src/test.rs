use super::*;
use denylist_gate::{DenylistGate, DenylistGateClient};
use jurisdiction_flag::{JurisdictionFlag, JurisdictionFlagClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{vec, Env, String};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Registers and initialises a denylist-gate contract, returns its address.
fn setup_denylist(env: &Env, admin: &Address) -> Address {
    let id = env.register(DenylistGate, ());
    DenylistGateClient::new(env, &id).initialize(admin);
    id
}

/// Registers and initialises a jurisdiction-flag contract, returns its address.
fn setup_jurisdiction(env: &Env, issuer: &Address) -> Address {
    let id = env.register(JurisdictionFlag, ());
    JurisdictionFlagClient::new(env, &id).initialize(issuer);
    id
}

/// Registers and initialises a policy-engine contract with `All` semantics,
/// returns `(admin, contract_id, client)`.
fn setup_engine_all(env: &Env) -> (Address, Address, PolicyEngineClient<'_>) {
    let admin = Address::generate(env);
    let id = env.register(PolicyEngine, ());
    let client = PolicyEngineClient::new(env, &id);
    client.initialize(&admin, &CombineOp::All);
    (admin, id, client)
}

/// Same as above but with `Any` semantics.
fn setup_engine_any(env: &Env) -> (Address, Address, PolicyEngineClient<'_>) {
    let admin = Address::generate(env);
    let id = env.register(PolicyEngine, ());
    let client = PolicyEngineClient::new(env, &id);
    client.initialize(&admin, &CombineOp::Any);
    (admin, id, client)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// With All semantics: a denylist check (addresses clear) and a jurisdiction
/// check (addresses in permitted list) both pass → evaluate returns true.
#[test]
fn test_all_checks_pass() {
    let env = Env::default();
    env.mock_all_auths();

    let deny_admin = Address::generate(&env);
    let juri_issuer = Address::generate(&env);
    let deny_id = setup_denylist(&env, &deny_admin);
    let juri_id = setup_jurisdiction(&env, &juri_issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Set permitted jurisdictions for both addresses.
    let code_us = String::from_str(&env, "US");
    JurisdictionFlagClient::new(&env, &juri_id)
        .set_jurisdiction(&juri_issuer, &from, &code_us);
    JurisdictionFlagClient::new(&env, &juri_id)
        .set_jurisdiction(&juri_issuer, &to, &code_us);

    let (admin, _engine_id, client) = setup_engine_all(&env);

    client.add_check(
        &admin,
        &CheckKind::Denylist {
            contract: deny_id.clone(),
        },
    );
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction {
            contract: juri_id.clone(),
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        },
    );

    let result = client.evaluate(&from, &to);
    assert!(result);
}

/// With All semantics: one of two checks fails (sender is on the denylist)
/// → evaluate returns false.
#[test]
fn test_one_check_fails_and_semantics() {
    let env = Env::default();
    env.mock_all_auths();

    let deny_admin = Address::generate(&env);
    let juri_issuer = Address::generate(&env);
    let deny_id = setup_denylist(&env, &deny_admin);
    let juri_id = setup_jurisdiction(&env, &juri_issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Both addresses have valid jurisdiction codes.
    let code_us = String::from_str(&env, "US");
    JurisdictionFlagClient::new(&env, &juri_id)
        .set_jurisdiction(&juri_issuer, &from, &code_us);
    JurisdictionFlagClient::new(&env, &juri_id)
        .set_jurisdiction(&juri_issuer, &to, &code_us);

    // But `from` is denied.
    DenylistGateClient::new(&env, &deny_id).add_to_denylist(&deny_admin, &from);

    let (admin, _engine_id, client) = setup_engine_all(&env);

    client.add_check(
        &admin,
        &CheckKind::Denylist {
            contract: deny_id.clone(),
        },
    );
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction {
            contract: juri_id.clone(),
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        },
    );

    let result = client.evaluate(&from, &to);
    assert!(!result);
}

/// With Any semantics: the denylist check fails (both addresses denied) but
/// the jurisdiction check passes → evaluate returns true because at least
/// one check passes for both parties.
#[test]
fn test_one_check_passes_or_semantics() {
    let env = Env::default();
    env.mock_all_auths();

    let deny_admin = Address::generate(&env);
    let juri_issuer = Address::generate(&env);
    let deny_id = setup_denylist(&env, &deny_admin);
    let juri_id = setup_jurisdiction(&env, &juri_issuer);

    let from = Address::generate(&env);
    let to = Address::generate(&env);

    // Both addresses are on the denylist (denylist check will fail).
    DenylistGateClient::new(&env, &deny_id).add_to_denylist(&deny_admin, &from);
    DenylistGateClient::new(&env, &deny_id).add_to_denylist(&deny_admin, &to);

    // But both have valid jurisdiction codes (jurisdiction check will pass).
    let code_us = String::from_str(&env, "US");
    JurisdictionFlagClient::new(&env, &juri_id)
        .set_jurisdiction(&juri_issuer, &from, &code_us);
    JurisdictionFlagClient::new(&env, &juri_id)
        .set_jurisdiction(&juri_issuer, &to, &code_us);

    let (admin, _engine_id, client) = setup_engine_any(&env);

    client.add_check(
        &admin,
        &CheckKind::Denylist {
            contract: deny_id.clone(),
        },
    );
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction {
            contract: juri_id.clone(),
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        },
    );

    // With Any: the jurisdiction check passes for both → result is true.
    let result = client.evaluate(&from, &to);
    assert!(result);
}

/// Verify that add_check and remove_check correctly mutate the checks list.
#[test]
fn test_add_and_remove_check() {
    let env = Env::default();
    env.mock_all_auths();

    let deny_admin = Address::generate(&env);
    let deny_id = setup_denylist(&env, &deny_admin);

    let (admin, _engine_id, client) = setup_engine_all(&env);

    // Initially empty.
    assert_eq!(client.get_checks().len(), 0);

    // Add one check.
    client.add_check(
        &admin,
        &CheckKind::Denylist {
            contract: deny_id.clone(),
        },
    );
    assert_eq!(client.get_checks().len(), 1);

    // Add a second check.
    let juri_issuer = Address::generate(&env);
    let juri_id = setup_jurisdiction(&env, &juri_issuer);
    client.add_check(
        &admin,
        &CheckKind::Jurisdiction {
            contract: juri_id.clone(),
            allowed_codes: vec![&env, String::from_str(&env, "US")],
        },
    );
    assert_eq!(client.get_checks().len(), 2);

    // Remove the first check (index 0); list should shrink to 1.
    client.remove_check(&admin, &0);
    assert_eq!(client.get_checks().len(), 1);
}

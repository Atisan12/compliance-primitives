use super::*;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{vec, Env, IntoVal, Map, Symbol, Val};

fn setup(env: &Env) -> (Address, Address, JurisdictionFlagClient<'_>) {
    env.mock_all_auths();
    let issuer = Address::generate(env);
    let contract_id = env.register(JurisdictionFlag, ());
    let client = JurisdictionFlagClient::new(env, &contract_id);
    client.initialize(&issuer);
    (issuer, contract_id, client)
}

// ── existing tests (unchanged) ────────────────────────────────────────────────

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
    assert_eq!(client.is_permitted_jurisdiction(&alice, &allowed), true);
}

#[test]
fn test_is_permitted_jurisdiction_false_when_no_jurisdiction_set() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let allowed = vec![&env, String::from_str(&env, "US")];
    assert_eq!(client.is_permitted_jurisdiction(&alice, &allowed), false);
}

#[test]
fn test_is_permitted_jurisdiction_errors_with_empty_allowed_list() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");
    client.set_jurisdiction(&issuer, &alice, &code);

    let allowed: Vec<String> = vec![&env];
    let result = client.try_is_permitted_jurisdiction(&alice, &allowed);
    assert_eq!(result, Err(Ok(Error::EmptyAllowedCodes)));
}

#[test]
fn test_is_permitted_jurisdiction_errors_when_no_jurisdiction_and_empty_allowed_list() {
    let env = Env::default();
    let (_issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    let allowed: Vec<String> = vec![&env];
    let result = client.try_is_permitted_jurisdiction(&alice, &allowed);
    assert_eq!(result, Err(Ok(Error::EmptyAllowedCodes)));
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
fn test_set_jurisdiction_emits_jurisdiction_set_event() {
    let env = Env::default();
    let (issuer, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "US");

    client.set_jurisdiction(&issuer, &alice, &code);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "jurisdiction_set"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::from_array(
                    &env,
                    [(Symbol::new(&env, "code"), code.clone().into_val(&env))]
                )
                .into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let result = client.try_initialize(&issuer);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

// ── new time-bound tests ──────────────────────────────────────────────────────

/// A flag set with `set_jurisdiction_until` is readable and permitted before
/// its `valid_until` ledger sequence is reached.
#[test]
fn test_set_jurisdiction_until_valid_before_expiry() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "DE");

    // Set ledger sequence well before the expiry.
    env.ledger().with_mut(|li| li.sequence_number = 100);
    client.set_jurisdiction_until(&issuer, &alice, &code, &200_u32);

    // Still before expiry — flag should be present and permitted.
    env.ledger().with_mut(|li| li.sequence_number = 150);
    assert_eq!(client.get_jurisdiction(&alice), Some(code.clone()));

    let allowed = vec![&env, String::from_str(&env, "DE")];
    assert!(client.is_permitted_jurisdiction(&alice, &allowed));
}

/// A flag set with `set_jurisdiction_until` is treated as unset once the
/// current ledger sequence strictly exceeds `valid_until`.
#[test]
fn test_set_jurisdiction_until_expired_after_expiry() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "FR");

    env.ledger().with_mut(|li| li.sequence_number = 100);
    client.set_jurisdiction_until(&issuer, &alice, &code, &200_u32);

    // Past expiry — flag should be treated as unset.
    env.ledger().with_mut(|li| li.sequence_number = 201);
    assert_eq!(client.get_jurisdiction(&alice), None);

    let allowed = vec![&env, String::from_str(&env, "FR")];
    assert!(!client.is_permitted_jurisdiction(&alice, &allowed));
}

/// At the exact `valid_until` ledger sequence the flag is still valid
/// (`valid_until` is inclusive).
#[test]
fn test_set_jurisdiction_until_boundary_ledger() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let code = String::from_str(&env, "JP");

    env.ledger().with_mut(|li| li.sequence_number = 100);
    client.set_jurisdiction_until(&issuer, &alice, &code, &200_u32);

    // Exactly at valid_until — flag should still be valid.
    env.ledger().with_mut(|li| li.sequence_number = 200);
    assert_eq!(client.get_jurisdiction(&alice), Some(code.clone()));

    let allowed = vec![&env, String::from_str(&env, "JP")];
    assert!(client.is_permitted_jurisdiction(&alice, &allowed));
}

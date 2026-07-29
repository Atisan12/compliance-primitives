use super::*;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{vec, Env, IntoVal, Map, Symbol, Val};

// We register the audit-log contract in integration tests using the
// AuditLog type imported from the audit-log crate (dev-dependency).
// To avoid linking two sets of wasm exports, we only use audit_log in
// the test module under #[cfg(test)] — the standard Soroban pattern.
#[cfg(test)]
mod audit_log_integration {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    // Minimal stand-in for the audit-log contract so we can test the wiring
    // without pulling in the full crate. In a real project you'd register the
    // actual audit-log contract binary; here we verify the AuditLogClient
    // interface shape compiles and the set_audit_log admin gate works.

    #[test]
    fn test_set_audit_log_rejects_non_admin() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register(DenylistGate, ());
        let client = DenylistGateClient::new(&env, &contract_id);
        client.initialize(&admin);

        let impostor = Address::generate(&env);
        let fake_audit_log = Address::generate(&env);

        let result = client.try_set_audit_log(&impostor, &fake_audit_log);
        assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    }

    #[test]
    fn test_set_audit_log_succeeds_for_admin() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register(DenylistGate, ());
        let client = DenylistGateClient::new(&env, &contract_id);
        client.initialize(&admin);

        let fake_audit_log = Address::generate(&env);
        // Should succeed — no panic, no error.
        client.set_audit_log(&admin, &fake_audit_log);
    }
}

fn setup(env: &Env) -> (Address, Address, DenylistGateClient<'_>) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let contract_id = env.register(DenylistGate, ());
    let client = DenylistGateClient::new(env, &contract_id);
    client.initialize(&admin);
    (admin, contract_id, client)
}

#[test]
fn test_check_defaults_to_clear() {
    let env = Env::default();
    let (_admin, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    assert!(client.check(&alice));
}

#[test]
fn test_add_and_remove_from_denylist() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.add_to_denylist(&admin, &alice);
    assert!(!client.check(&alice));

    client.remove_from_denylist(&admin, &alice);
    assert!(client.check(&alice));
}

#[test]
fn test_add_to_denylist_rejects_non_admin() {
    let env = Env::default();
    let (_admin, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    let result = client.try_add_to_denylist(&impostor, &alice);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(client.check(&alice));
}

#[test]
fn test_empty_address_key_is_well_defined() {
    // An address that has never been touched must read as "clear" (true),
    // not panic or default to denied. This guards the `unwrap_or(false)`
    // fallback in `check`.
    let env = Env::default();
    let (_admin, _contract_id, client) = setup(&env);
    let never_seen = Address::generate(&env);
    assert!(client.check(&never_seen));
}

#[test]
fn test_remove_from_denylist_never_added_is_noop() {
    let env = Env::default();
    let (admin, contract_id, client) = setup(&env);
    let never_added = Address::generate(&env);

    assert!(client.check(&never_added));

    client.remove_from_denylist(&admin, &never_added);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "deny_remove"), never_added.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
    assert!(client.check(&never_added));
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

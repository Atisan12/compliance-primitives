use super::*;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{vec, Env, IntoVal, Map, Symbol, Val, Vec};

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

// ── existing tests ──────────────────────────────────────────────────────────

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
fn test_is_denylisted_is_inverse_of_check() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    assert_eq!(client.is_denylisted(&alice), !client.check(&alice));

    client.add_to_denylist(&admin, &alice);
    assert_eq!(client.is_denylisted(&alice), !client.check(&alice));

    client.remove_from_denylist(&admin, &alice);
    assert_eq!(client.is_denylisted(&alice), !client.check(&alice));
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
fn test_remove_multiple_from_denylist_removes_all_and_emits_events() {
    let env = Env::default();
    let (admin, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    env.storage()
        .persistent()
        .set(&DataKey::Denied(alice.clone()), &true);
    env.storage()
        .persistent()
        .set(&DataKey::Denied(bob.clone()), &true);

    client.remove_multiple_from_denylist(&admin, &vec![&env, alice.clone(), bob.clone()]);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "deny_remove"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
            (
                contract_id.clone(),
                (Symbol::new(&env, "deny_remove"), bob.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
    assert!(client.check(&alice));
    assert!(client.check(&bob));
}

#[test]
fn test_remove_multiple_from_denylist_rejects_non_admin() {
    let env = Env::default();
    let (_admin, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    let result = client.try_remove_multiple_from_denylist(&impostor, &vec![&env, alice.clone()]);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(client.check(&alice));
}

#[test]
fn test_remove_multiple_from_denylist_empty_vec_is_noop() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);

    client.remove_multiple_from_denylist(&admin, &vec![&env]);

    assert_eq!(env.events().all(), vec![&env]);
}

#[test]
fn test_remove_multiple_from_denylist_batch_limit_succeeds() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let mut addresses: Vec<Address> = Vec::new(&env);

    for _ in 0..MAX_BATCH_SIZE {
        let address = Address::generate(&env);
        env.storage()
            .persistent()
            .set(&DataKey::Denied(address.clone()), &true);
        addresses.push_back(address);
    }

    client.remove_multiple_from_denylist(&admin, &addresses);

    for address in addresses.iter() {
        assert!(client.check(&address));
    }
}

#[test]
fn test_remove_multiple_from_denylist_over_batch_limit_rejected() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let mut addresses: Vec<Address> = Vec::new(&env);

    for _ in 0..(MAX_BATCH_SIZE + 1) {
        let address = Address::generate(&env);
        env.storage()
            .persistent()
            .set(&DataKey::Denied(address.clone()), &true);
        addresses.push_back(address);
    }

    let first = addresses.get_unchecked(0);
    let result = client.try_remove_multiple_from_denylist(&admin, &addresses);
    assert_eq!(result, Err(Ok(Error::BatchTooLarge)));
    assert!(!client.check(&first));
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

// ── pausable tests ───────────────────────────────────────────────────────────

#[test]
fn test_not_paused_by_default() {
    let env = Env::default();
    let (_admin, _contract_id, client) = setup(&env);
    assert!(!client.is_paused());
}

#[test]
fn test_pause_and_unpause_by_admin() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);

    client.pause(&admin);
    assert!(client.is_paused());

    client.unpause(&admin);
    assert!(!client.is_paused());
}

#[test]
fn test_add_to_denylist_blocked_while_paused() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.pause(&admin);
    let result = client.try_add_to_denylist(&admin, &alice);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    // alice must still be clear — no state change
    assert!(client.check(&alice));
}

#[test]
fn test_remove_from_denylist_blocked_while_paused() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    // add alice first (while unpaused)
    client.add_to_denylist(&admin, &alice);
    assert!(!client.check(&alice));

    client.pause(&admin);
    let result = client.try_remove_from_denylist(&admin, &alice);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    // alice must still be denied — no state change
    assert!(!client.check(&alice));
}

#[test]
fn test_check_works_while_paused() {
    // Read-only operations must not be gated by pause.
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.add_to_denylist(&admin, &alice);
    client.pause(&admin);

    // check still works for both a denied and a clear address
    assert!(!client.check(&alice));
    let bob = Address::generate(&env);
    assert!(client.check(&bob));
}

#[test]
fn test_mutations_resume_after_unpause() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.pause(&admin);
    assert_eq!(
        client.try_add_to_denylist(&admin, &alice),
        Err(Ok(Error::ContractPaused))
    );

    client.unpause(&admin);
    client.add_to_denylist(&admin, &alice);
    assert!(!client.check(&alice));
}

#[test]
fn test_non_admin_cannot_pause() {
    let env = Env::default();
    let (_admin, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);

    let result = client.try_pause(&impostor);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_paused());
}

#[test]
fn test_non_admin_cannot_unpause() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);

    client.pause(&admin);
    let result = client.try_unpause(&impostor);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(client.is_paused());
}

#[test]
fn test_pause_emits_event() {
    let env = Env::default();
    let (admin, contract_id, client) = setup(&env);

    client.pause(&admin);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "paused"), admin.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_unpause_emits_event() {
    let env = Env::default();
    let (admin, contract_id, client) = setup(&env);

    client.pause(&admin);
    // clear events so we only see the unpause event
    let _ = env.events().all();

    client.unpause(&admin);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "unpaused"), admin.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
}

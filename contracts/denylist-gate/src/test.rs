use super::*;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{vec, Env, IntoVal, Map, Symbol, Val};

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
fn test_check_true_immediately_after_remove_from_denylist() {
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
fn test_add_to_denylist_twice_is_idempotent() {
    // Adding the same address twice should succeed both times (storage
    // overwrite is a no-op) and leave the address denied. Each call still
    // emits its own DenyAdd event because the contract has no dedup logic —
    // two calls, two events.
    let env = Env::default();
    let (admin, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.add_to_denylist(&admin, &alice);
    client.add_to_denylist(&admin, &alice);

    assert!(!client.check(&alice));

    let deny_add_topic: Val = (Symbol::new(&env, "deny_add"), alice.clone()).into_val(&env);
    let empty: Val = Map::<Symbol, Val>::new(&env).into_val(&env);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (contract_id.clone(), deny_add_topic.clone(), empty.clone()),
            (contract_id.clone(), deny_add_topic.clone(), empty.clone()),
        ]
    );
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

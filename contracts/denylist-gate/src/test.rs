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

// ── compliance-officer tests ───────────────────────────────────────────

#[test]
fn test_admin_can_set_and_revoke_compliance_officer() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);

    // Set compliance officer
    client.set_compliance_officer(&admin, &officer);

    // Officer can add to denylist
    let alice = Address::generate(&env);
    client.add_to_denylist(&officer, &alice);
    assert!(!client.check(&alice));

    // Revoke compliance officer
    client.revoke_compliance_officer(&admin);

    // Now officer cannot add another address
    let bob = Address::generate(&env);
    let result = client.try_add_to_denylist(&officer, &bob);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(client.check(&bob));
}

#[test]
fn test_compliance_officer_can_add_and_remove() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&admin, &officer);

    let alice = Address::generate(&env);

    // Officer can add
    client.add_to_denylist(&officer, &alice);
    assert!(!client.check(&alice));

    // Officer can remove
    client.remove_from_denylist(&officer, &alice);
    assert!(client.check(&alice));
}

#[test]
fn test_compliance_officer_cannot_set_or_revoke_role() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&admin, &officer);

    let another = Address::generate(&env);

    // Officer cannot set another compliance officer
    let result = client.try_set_compliance_officer(&officer, &another);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));

    // Officer cannot revoke own role
    let result = client.try_revoke_compliance_officer(&officer);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));

    // Officer still has their role
    let alice = Address::generate(&env);
    client.add_to_denylist(&officer, &alice);
    assert!(!client.check(&alice));
}

#[test]
fn test_admin_can_still_perform_compliance_actions() {
    let env = Env::default();
    let (admin, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&admin, &officer);

    let alice = Address::generate(&env);

    // Admin can still add/remove directly
    client.add_to_denylist(&admin, &alice);
    assert!(!client.check(&alice));

    client.remove_from_denylist(&admin, &alice);
    assert!(client.check(&alice));
}

#[test]
fn test_unset_officer_rejected_for_compliance_actions() {
    let env = Env::default();
    let (_admin, _contract_id, client) = setup(&env);

    // No compliance officer set — unknown address cannot act
    let rando = Address::generate(&env);
    let alice = Address::generate(&env);
    let result = client.try_add_to_denylist(&rando, &alice);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(client.check(&alice));
}

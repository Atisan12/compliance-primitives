use super::*;
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{contract, contractimpl, symbol_short, vec, Env, IntoVal, Map, Symbol, Val};

/// A minimal token double used only by these tests, so `allowlist-token`'s
/// unit tests don't depend on any particular real SEP-41 implementation.
#[contract]
struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "last"), &(from, to, amount));
    }

    pub fn last_transfer(env: Env) -> Option<(Address, Address, i128)> {
        env.storage().instance().get(&Symbol::new(&env, "last"))
    }
}

fn setup(env: &Env) -> (Address, Address, Address, AllowlistTokenClient<'_>) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let token_id = env.register(MockToken, ());
    let contract_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(env, &contract_id);
    client.initialize(&admin, &token_id);
    (admin, token_id, contract_id, client)
}

#[test]
fn test_initialize_and_allowlist_roundtrip() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    assert!(!client.is_allowed(&alice));
    client.add_to_allowlist(&admin, &alice);
    assert!(client.is_allowed(&alice));
    client.remove_from_allowlist(&admin, &alice);
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_transfer_forwards_to_underlying_token_when_both_allowlisted() {
    let env = Env::default();
    let (admin, token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);
    client.add_to_allowlist(&admin, &bob);

    let ok = client.transfer(&alice, &bob, &500);
    assert!(ok);

    let token_client = MockTokenClient::new(&env, &token_id);
    let last = token_client.last_transfer().unwrap();
    assert_eq!(last, (alice, bob, 500));
}

#[test]
fn test_transfer_blocked_when_recipient_not_allowlisted() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);

    let ok = client.transfer(&alice, &bob, &500);
    assert!(!ok);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (symbol_short!("blocked"), alice.clone(), bob.clone()).into_val(&env),
                Map::<Symbol, Val>::from_array(
                    &env,
                    [(symbol_short!("amount"), 500i128.into_val(&env))]
                )
                .into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_add_to_allowlist_rejects_non_admin() {
    let env = Env::default();
    let (_admin, _token_id, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    let result = client.try_add_to_allowlist(&impostor, &alice);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_non_admin_allowlist_mutations_rejected_end_to_end() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);
    let alice = Address::generate(&env);

    let add_result = client.try_add_to_allowlist(&impostor, &alice);
    assert_eq!(add_result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_allowed(&alice));

    client.add_to_allowlist(&admin, &alice);
    assert!(client.is_allowed(&alice));

    let remove_result = client.try_remove_from_allowlist(&impostor, &alice);
    assert_eq!(remove_result, Err(Ok(Error::NotAuthorized)));
    assert!(client.is_allowed(&alice));
}

#[test]
fn test_remove_from_allowlist_never_added_is_noop() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let never_added = Address::generate(&env);

    assert!(!client.is_allowed(&never_added));

    client.remove_from_allowlist(&admin, &never_added);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_remove"), never_added.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
    assert!(!client.is_allowed(&never_added));
}

#[test]
fn test_is_allowed_false_before_initialize() {
    let env = Env::default();
    let contract_id = env.register(AllowlistToken, ());
    let client = AllowlistTokenClient::new(&env, &contract_id);
    let alice = Address::generate(&env);

    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_double_initialize_fails() {
    let env = Env::default();
    let (admin, token_id, _contract_id, client) = setup(&env);
    let result = client.try_initialize(&admin, &token_id);
    assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
}

#[test]
fn test_add_to_allowlist_emits_allow_add_event() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_add"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_remove_from_allowlist_emits_allow_remove_event() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    client.add_to_allowlist(&admin, &alice);

    client.remove_from_allowlist(&admin, &alice);

    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                contract_id.clone(),
                (Symbol::new(&env, "allow_remove"), alice.clone()).into_val(&env),
                Map::<Symbol, Val>::new(&env).into_val(&env),
            ),
        ]
    );
}

// ── compliance-officer tests ───────────────────────────────────────────

#[test]
fn test_admin_can_set_and_revoke_compliance_officer() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);

    // Set compliance officer
    client.set_compliance_officer(&admin, &officer);

    // Officer can add to allowlist
    let alice = Address::generate(&env);
    client.add_to_allowlist(&officer, &alice);
    assert!(client.is_allowed(&alice));

    // Revoke compliance officer
    client.revoke_compliance_officer(&admin);

    // Now officer cannot add another address
    let bob = Address::generate(&env);
    let result = client.try_add_to_allowlist(&officer, &bob);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_allowed(&bob));
}

#[test]
fn test_compliance_officer_can_add_and_remove() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&admin, &officer);

    let alice = Address::generate(&env);

    // Officer can add
    client.add_to_allowlist(&officer, &alice);
    assert!(client.is_allowed(&alice));

    // Officer can remove
    client.remove_from_allowlist(&officer, &alice);
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_compliance_officer_cannot_set_or_revoke_role() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
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
    client.add_to_allowlist(&officer, &alice);
    assert!(client.is_allowed(&alice));
}

#[test]
fn test_admin_can_still_perform_compliance_actions() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let officer = Address::generate(&env);
    client.set_compliance_officer(&admin, &officer);

    let alice = Address::generate(&env);

    // Admin can still add/remove directly
    client.add_to_allowlist(&admin, &alice);
    assert!(client.is_allowed(&alice));

    client.remove_from_allowlist(&admin, &alice);
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_unset_officer_rejected_for_compliance_actions() {
    let env = Env::default();
    let (_admin, _token_id, _contract_id, client) = setup(&env);

    // No compliance officer set — unknown address cannot act
    let rando = Address::generate(&env);
    let alice = Address::generate(&env);
    let result = client.try_add_to_allowlist(&rando, &alice);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_allowed(&alice));
}

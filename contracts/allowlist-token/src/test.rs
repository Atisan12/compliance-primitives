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

// ── existing tests ──────────────────────────────────────────────────────────

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

// ── pausable tests ───────────────────────────────────────────────────────────

#[test]
fn test_not_paused_by_default() {
    let env = Env::default();
    let (_admin, _token_id, _contract_id, client) = setup(&env);
    assert!(!client.is_paused());
}

#[test]
fn test_pause_and_unpause_by_admin() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);

    client.pause(&admin);
    assert!(client.is_paused());

    client.unpause(&admin);
    assert!(!client.is_paused());
}

#[test]
fn test_add_to_allowlist_blocked_while_paused() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.pause(&admin);
    let result = client.try_add_to_allowlist(&admin, &alice);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    // no state change
    assert!(!client.is_allowed(&alice));
}

#[test]
fn test_remove_from_allowlist_blocked_while_paused() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);
    client.pause(&admin);

    let result = client.try_remove_from_allowlist(&admin, &alice);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
    // alice must still be allowed — no state change
    assert!(client.is_allowed(&alice));
}

#[test]
fn test_transfer_blocked_while_paused() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);
    client.add_to_allowlist(&admin, &bob);
    client.pause(&admin);

    let result = client.try_transfer(&alice, &bob, &100);
    assert_eq!(result, Err(Ok(Error::ContractPaused)));
}

#[test]
fn test_is_allowed_works_while_paused() {
    // Read-only operations must not be gated by pause.
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);
    client.pause(&admin);

    assert!(client.is_allowed(&alice));
    let bob = Address::generate(&env);
    assert!(!client.is_allowed(&bob));
}

#[test]
fn test_mutations_resume_after_unpause() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);

    client.pause(&admin);
    assert_eq!(
        client.try_add_to_allowlist(&admin, &alice),
        Err(Ok(Error::ContractPaused))
    );

    client.unpause(&admin);
    client.add_to_allowlist(&admin, &alice);
    assert!(client.is_allowed(&alice));
}

#[test]
fn test_non_admin_cannot_pause() {
    let env = Env::default();
    let (_admin, _token_id, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);

    let result = client.try_pause(&impostor);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_paused());
}

#[test]
fn test_non_admin_cannot_unpause() {
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);

    client.pause(&admin);
    let result = client.try_unpause(&impostor);
    assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    assert!(client.is_paused());
}

#[test]
fn test_pause_emits_event() {
    let env = Env::default();
    let (admin, _token_id, contract_id, client) = setup(&env);

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
    let (admin, _token_id, contract_id, client) = setup(&env);

    client.pause(&admin);
    let _ = env.events().all(); // clear

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

#[test]
fn test_transfer_paused_produces_no_blocked_event() {
    // When the contract is paused, transfer returns Err(ContractPaused) which
    // rolls back all state including events. The Blocked event must NOT land.
    let env = Env::default();
    let (admin, _token_id, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    client.add_to_allowlist(&admin, &alice);
    client.add_to_allowlist(&admin, &bob);
    client.pause(&admin);

    let _ = env.events().all(); // clear pause/add events

    let _ = client.try_transfer(&alice, &bob, &100);
    // no events should have been emitted (the call errored out)
    assert_eq!(env.events().all(), vec![&env]);
}

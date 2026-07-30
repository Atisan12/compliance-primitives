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

    let allowed = vec![&env, String::from_str(&env, "CA"), String::from_str(&env, "US")];
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

#[test]
fn test_multiple_jurisdictions_add_remove_and_list() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let us = String::from_str(&env, "US");
    let ca = String::from_str(&env, "CA");
    let gb = String::from_str(&env, "GB");

    client.add_jurisdiction(&issuer, &alice, &us);
    client.add_jurisdiction(&issuer, &alice, &ca);
    client.add_jurisdiction(&issuer, &alice, &gb);

    let listed = client.list_jurisdictions(&alice);
    assert_eq!(listed.len(), 3);
    assert_eq!(listed.get(0).unwrap(), us);
    assert_eq!(listed.get(1).unwrap(), ca);
    assert_eq!(listed.get(2).unwrap(), gb);
    // get_jurisdiction stays a single-code convenience: first code.
    assert_eq!(client.get_jurisdiction(&alice), Some(us.clone()));

    client.remove_jurisdiction(&issuer, &alice, &ca);
    let listed = client.list_jurisdictions(&alice);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed.get(0).unwrap(), us);
    assert_eq!(listed.get(1).unwrap(), gb);

    // Removing the last remaining of a set still works.
    client.remove_jurisdiction(&issuer, &alice, &us);
    client.remove_jurisdiction(&issuer, &alice, &gb);
    assert!(client.list_jurisdictions(&alice).is_empty());
    assert_eq!(client.get_jurisdiction(&alice), None);
}

#[test]
fn test_remove_jurisdiction_not_found() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_jurisdiction(&issuer, &alice, &us);

    let result = client.try_remove_jurisdiction(&issuer, &alice, &String::from_str(&env, "CA"));
    assert_eq!(result, Err(Ok(Error::NotFound)));
    assert_eq!(client.list_jurisdictions(&alice).len(), 1);
}

#[test]
fn test_set_jurisdiction_replaces_multi_code_set() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    client.add_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));
    client.add_jurisdiction(&issuer, &alice, &String::from_str(&env, "CA"));

    let only = String::from_str(&env, "GB");
    client.set_jurisdiction(&issuer, &alice, &only);
    let listed = client.list_jurisdictions(&alice);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed.get(0).unwrap(), only);
}

#[test]
fn test_is_permitted_any_semantics_with_multi_code_address() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    // Dual residency: US + IR. Allowed list only includes US — "any" match.
    client.add_jurisdiction(&issuer, &alice, &String::from_str(&env, "US"));
    client.add_jurisdiction(&issuer, &alice, &String::from_str(&env, "IR"));

    let allowed_us = vec![&env, String::from_str(&env, "US")];
    assert!(client.is_permitted_jurisdiction(&alice, &allowed_us));

    let allowed_ca = vec![&env, String::from_str(&env, "CA")];
    assert!(!client.is_permitted_jurisdiction(&alice, &allowed_ca));

    // "all" would require every address code to be allowed; we intentionally
    // do NOT require that — IR is not in allowed_us, but US is, so true.
}

#[test]
fn test_add_jurisdiction_idempotent() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let us = String::from_str(&env, "US");
    client.add_jurisdiction(&issuer, &alice, &us);
    client.add_jurisdiction(&issuer, &alice, &us);
    assert_eq!(client.list_jurisdictions(&alice).len(), 1);
}

#[test]
fn test_pause_blocks_writes_reads_unaffected() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let alice = Address::generate(&env);
    let us = String::from_str(&env, "US");
    let ca = String::from_str(&env, "CA");

    client.set_jurisdiction(&issuer, &alice, &us);
    assert!(!client.is_paused());

    client.pause(&issuer);
    assert!(client.is_paused());

    // Writes blocked.
    assert_eq!(
        client.try_set_jurisdiction(&issuer, &alice, &ca),
        Err(Ok(Error::Paused))
    );
    assert_eq!(
        client.try_add_jurisdiction(&issuer, &alice, &ca),
        Err(Ok(Error::Paused))
    );
    assert_eq!(
        client.try_remove_jurisdiction(&issuer, &alice, &us),
        Err(Ok(Error::Paused))
    );

    // Reads still work against pre-pause state.
    assert_eq!(client.get_jurisdiction(&alice), Some(us.clone()));
    assert_eq!(client.list_jurisdictions(&alice).len(), 1);
    let allowed = vec![&env, us.clone()];
    assert!(client.is_permitted_jurisdiction(&alice, &allowed));

    client.unpause(&issuer);
    assert!(!client.is_paused());
    client.add_jurisdiction(&issuer, &alice, &ca);
    assert_eq!(client.list_jurisdictions(&alice).len(), 2);
}

#[test]
fn test_pause_unpause_rejects_non_issuer() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);
    let impostor = Address::generate(&env);

    assert_eq!(client.try_pause(&impostor), Err(Ok(Error::NotAuthorized)));
    assert!(!client.is_paused());

    client.pause(&issuer);
    assert_eq!(client.try_unpause(&impostor), Err(Ok(Error::NotAuthorized)));
    assert!(client.is_paused());
}

#[test]
fn test_pause_and_unpause_emit_events() {
    let env = Env::default();
    let (issuer, _contract_id, client) = setup(&env);

    assert_eq!(env.events().all(), vec![&env]);

    client.pause(&issuer);
    assert_ne!(env.events().all(), vec![&env]);

    let events_after_pause = env.events().all();
    client.unpause(&issuer);
    assert_ne!(env.events().all(), events_after_pause);
}

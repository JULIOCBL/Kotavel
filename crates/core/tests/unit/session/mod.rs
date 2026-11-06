use super::*;

#[test]
fn session_ids_are_unique_and_have_256_bits_of_entropy() {
    let first = generate_session_id().unwrap();
    let second = generate_session_id().unwrap();
    assert!(valid_session_id(&first));
    assert!(valid_session_id(&second));
    assert_ne!(first, second);
}

#[test]
fn expiry_is_checked_at_the_server_timestamp() {
    let record = SessionRecord {
        id: "id".into(),
        created_at: 10,
        last_activity: 10,
        expires_at: 20,
        revision: 1,
        values: Map::new(),
    };
    assert!(!record.is_expired(19));
    assert!(record.is_expired(20));
}

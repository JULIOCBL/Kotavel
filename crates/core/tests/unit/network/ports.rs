use super::DISCOVERY_PORT_CANDIDATES;

#[test]
fn discovery_listener_candidates_keep_the_documented_order() {
    assert_eq!(
        DISCOVERY_PORT_CANDIDATES,
        &[
            3000, 5000, 8000, 8080, 9000, 28474, 28475, 28476, 28477, 28478
        ]
    );
}

use super::*;

/// A store that cannot exceed its cap, and says so when it clears.
#[test]
fn the_store_respects_its_cap() {
    let store = ClosednessStore::new(256);
    for i in 0..200 {
        // A certificate the store can size; content is irrelevant here.
        let cert = Arc::new(ClosednessCertificate::empty_for_test(&format!("C{i}")));
        store.put(&format!("Class{i}"), cert);
        assert!(
            store.resident_bytes() <= 256 + 4096,
            "store held {} bytes against a 256 cap — an unbounded derived \
             cache is a memory regression no functional test can see",
            store.resident_bytes()
        );
    }
}

/// A round trip: what goes in comes back out under the same key.
#[test]
fn a_stored_certificate_comes_back() {
    let store = ClosednessStore::new(1 << 20);
    assert!(store.get("Absent").is_none(), "an unknown class must miss");
    let cert = Arc::new(ClosednessCertificate::empty_for_test("Base"));
    store.put("Child", Arc::clone(&cert));
    assert_eq!(store.get("Child"), Some(cert), "a stored certificate must come back");
    assert_eq!(store.len(), 1);
}

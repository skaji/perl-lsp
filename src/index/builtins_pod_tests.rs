use super::*;

#[test]
fn bundled_perlfunc_yields_common_builtins() {
    let parsed = parse_bundled_perlfunc().expect("bundled POD parses");
    for name in ["push", "pop", "shift", "scalar", "keys", "values", "join", "split"] {
        assert!(
            parsed.entries.contains_key(name),
            "bundled perlfunc.pod should contain `{name}` (got {} entries)",
            parsed.entries.len(),
        );
    }
}

#[test]
fn entries_carry_version_footer() {
    let parsed = parse_bundled_perlfunc().expect("bundled POD parses");
    let push = parsed.entries.get("push").expect("push entry");
    assert!(
        push.contains("perl ") && push.contains("bundled"),
        "expected version footer in entry body, got tail: {}",
        push.chars().rev().take(80).collect::<String>().chars().rev().collect::<String>(),
    );
}

/// The anti-drift tripwire between the doc-VALUE store (perlfunc.pod) and
/// the ONE builtin surface (`model::builtins`): a perlfunc entry name the
/// table doesn't know is exactly the false-positive/undocumented-hover
/// asymmetry the parallel encodings used to produce.
///
/// `PERLFUNC_PROSE_NOISE` is the closed set of `=item` names the perlfunc
/// walker extracts that are NOT builtin names — deliberate exclusions, each
/// with its reason. Growing this set is a judgment call, not a formality.
#[test]
fn every_perlfunc_entry_is_on_the_builtin_surface() {
    // - flags/minimum/order/precision/size/vector: sprintf's format-attribute
    //   prose sub-items, not names.
    // - elseif: documented only to warn "it's spelled elsif".
    // - all/any: experimental keyword_all/keyword_any (5.42) — deliberately
    //   NOT suppressed so the List::Util import hint (the overwhelmingly
    //   common source of bare `any {...}`) keeps firing. Revisit on
    //   stabilization.
    const PERLFUNC_PROSE_NOISE: &[&str] = &[
        "all", "any", "elseif", "flags", "minimum", "order", "precision", "size", "vector",
    ];
    let parsed = parse_bundled_perlfunc().expect("bundled POD parses");
    let mut unknown: Vec<&str> = parsed
        .entries
        .keys()
        .map(|s| s.as_str())
        .filter(|n| !crate::model::builtins::is_builtin(n))
        .filter(|n| !PERLFUNC_PROSE_NOISE.contains(n))
        .collect();
    unknown.sort_unstable();
    assert!(
        unknown.is_empty(),
        "perlfunc.pod entries missing from model::builtins (add rows or extend \
         the documented noise set): {unknown:?}",
    );

    // Reverse direction: every callable row is documented — a Function row
    // perlfunc has never heard of is a typo'd or invented name.
    let mut undocumented: Vec<&str> = crate::model::builtins::builtin_functions()
        .filter(|n| !parsed.entries.contains_key(*n))
        .collect();
    undocumented.sort_unstable();
    assert!(
        undocumented.is_empty(),
        "Function rows in model::builtins with no perlfunc doc entry: {undocumented:?}",
    );
}

#[test]
fn push_body_describes_array_append() {
    let parsed = parse_bundled_perlfunc().expect("bundled POD parses");
    let push = parsed.entries.get("push").expect("push entry");
    assert!(
        push.to_lowercase().contains("array"),
        "push body should mention `array`, got: {push}"
    );
}

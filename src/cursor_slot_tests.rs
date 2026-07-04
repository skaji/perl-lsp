use super::*;

fn build(src: &str) -> (tree_sitter::Tree, FileAnalysis) {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_parser_perl::LANGUAGE.into()).unwrap();
    let tree = parser.parse(src, None).unwrap();
    let analysis = crate::builder::build(&tree, src.as_bytes());
    (tree, analysis)
}

/// The `expected_type()` lock: `ArgPosition` resolves the param type at a
/// call arg through the same witness-bag path sig-help's own param-type
/// rendering uses (`docs/adr/cursor-slots.md`'s stub — consumed by
/// nothing, this test is the guard against silent drift).
#[test]
fn expected_type_resolves_local_param_type_at_call_arg() {
    let src = r#"package Foo;
sub bar {
    my ($self, $x) = @_;
    $x = 5;
    return $x;
}

Foo->bar(1);
"#;
    let (_tree, analysis) = build(src);
    let callee = CalleeCtx {
        name: "bar".to_string(),
        is_method: true,
        invocant: Some("Foo".to_string()),
        active_param: 0,
        at_key_position: false,
        used_keys: Default::default(),
        first_arg_string: None,
    };
    let slot = Slot::ArgPosition { callee: Some(callee), index: 0 };
    let point = Point::new(0, 0);
    let ty = slot.expected_type(&analysis, point, None);
    assert_eq!(ty, Some(InferredType::Numeric), "param $x's last assignment types it Numeric: {:?}", ty);
}

/// Every other slot answers `None` — the stub has no other consumer yet.
#[test]
fn expected_type_is_none_off_arg_position() {
    let src = "package Foo;\nsub bar { 1 }\n";
    let (_tree, analysis) = build(src);
    let slot = Slot::Identifier { prefix: "fo".to_string() };
    assert_eq!(slot.expected_type(&analysis, Point::new(0, 0), None), None);

    let slot = Slot::ArgPosition { callee: None, index: 0 };
    assert_eq!(slot.expected_type(&analysis, Point::new(0, 0), None), None);
}

/// `Slot::sigil` decodes the bare-sigil-trigger fact back out of an
/// `Identifier` slot — the reconstruction `detect_slot`'s Perl fold
/// relies on to stay byte-identical with the old `CursorContext::Variable`
/// branch.
#[test]
fn sigil_decodes_bare_trigger_only() {
    assert_eq!(Slot::Identifier { prefix: "$".into() }.sigil(), Some('$'));
    assert_eq!(Slot::Identifier { prefix: "@".into() }.sigil(), Some('@'));
    assert_eq!(Slot::Identifier { prefix: "%".into() }.sigil(), Some('%'));
    assert_eq!(Slot::Identifier { prefix: "".into() }.sigil(), None);
    assert_eq!(Slot::Identifier { prefix: "foo".into() }.sigil(), None);
    assert_eq!(Slot::Identifier { prefix: "$x".into() }.sigil(), None);
    assert_eq!(
        Slot::Key { owner: OwnerCtx { owner_type: None, var_text: String::new(), source_sub: None } }
            .sigil(),
        None
    );
}

/// `detect_slot` on Perl reproduces the exact `CursorContext` verdicts:
/// method position after `->`.
#[test]
fn detect_slot_perl_method_position() {
    let src = "package Foo;\nsub greet { 1 }\npackage main;\nmy $f = bless {}, 'Foo';\n$f->\n";
    let (tree, analysis) = build(src);
    let point = Point::new(4, 4); // right after `$f->`
    let slot = detect_slot(&analysis, &tree, src, point, "perl", None);
    match slot {
        Slot::Member { receiver, op } => {
            assert_eq!(op, MemberOp::Arrow);
            assert_eq!(receiver.receiver_text.as_deref(), Some("$f"));
        }
        other => panic!("expected Member slot, got {:?}", other),
    }
}

/// `use |` (typing the module name) is `ModulePath` with `in_use: true` —
/// the fork this migration needed beyond the ADR's minimal sketch
/// (`docs/open-forks.md`).
#[test]
fn detect_slot_perl_use_module_name_is_module_path() {
    let src = "use Sc\n";
    let (tree, analysis) = build(src);
    let point = Point::new(0, 6);
    let slot = detect_slot(&analysis, &tree, src, point, "perl", None);
    match slot {
        Slot::ModulePath { prefix, in_use } => {
            assert_eq!(prefix, "Sc");
            assert!(in_use);
        }
        other => panic!("expected ModulePath slot, got {:?}", other),
    }
}

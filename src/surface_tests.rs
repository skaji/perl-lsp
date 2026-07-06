use super::*;

fn build(source: &str) -> crate::file_analysis::FileAnalysis {
    use tree_sitter::Parser;
    let mut parser = Parser::new();
    parser.set_language(&ts_parser_perl::LANGUAGE.into()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    crate::builder::build(&tree, source.as_bytes())
}

fn surface(source: &str) -> Surface {
    Surface::project(&build(source))
}

/// R1 regression net: edits with NO cross-file-visible effect yield an
/// EQUAL Surface — this equality is the freshness firewall. Every Surface
/// field addition needs an arm here.
#[test]
fn body_edits_reformat_and_comments_keep_the_surface_equal() {
    let base = "package Acme::W;\nuse List::Util qw(sum);\nour @EXPORT_OK = qw(area);\nsub area {\n    my ($self, $w) = @_;\n    return $w * 2;\n}\nsub _private_helper { my $x = 1; return $x }\n1;\n";
    let s0 = surface(base);

    // Body-only edit: different math, same contract.
    let body_edit = base.replace("return $w * 2;", "my $tmp = $w + $w;\n    return $tmp;");
    assert_ne!(base, body_edit);
    assert_eq!(s0, surface(&body_edit), "body edit must not change the surface");

    // Reformat: whitespace + comment padding shifts every span.
    let reformatted = "package Acme::W;\n\n# a comment banner\nuse List::Util qw(sum);\n\nour @EXPORT_OK = qw(area);\n\nsub area {\n        my ($self, $w) = @_;\n        # doubled\n        return $w * 2;\n}\n\nsub _private_helper { my $x = 1; return $x }\n1;\n";
    assert_eq!(s0, surface(reformatted), "reformat must not change the surface");

    // Renaming a body-local variable.
    let local_rename = base.replace("$x", "$y");
    assert_eq!(s0, surface(&local_rename), "local rename must not change the surface");
}

/// The inverse net: every cross-file-visible edit class must FLIP equality.
#[test]
fn surface_changing_edits_are_unequal() {
    let base = "package Acme::W;\nour @EXPORT_OK = qw(area);\nsub area { my ($self, $w) = @_; return $w * 2; }\n1;\n";
    let s0 = surface(base);

    // Return-type change (number -> hashref) — the outline-blind case.
    let ret_edit = base.replace("return $w * 2;", "return { w => $w };");
    assert_ne!(s0, surface(&ret_edit), "return-type change must change the surface");

    // New public sub.
    let add_sub = base.replace("1;\n", "sub perimeter { my ($self) = @_; return 0 }\n1;\n");
    assert_ne!(s0, surface(&add_sub), "added method must change the surface");

    // Parent change.
    let add_parent = base.replace(
        "package Acme::W;\n",
        "package Acme::W;\nuse parent 'Acme::Base';\n",
    );
    assert_ne!(s0, surface(&add_parent), "@ISA change must change the surface");

    // Export-list change.
    let add_export = base.replace("qw(area)", "qw(area area2)");
    assert_ne!(s0, surface(&add_export), "export change must change the surface");

    // New import (a freshness EDGE change even with no member change).
    let add_import = base.replace(
        "package Acme::W;\n",
        "package Acme::W;\nuse Scalar::Util qw(blessed);\n",
    );
    assert_ne!(s0, surface(&add_import), "import change must change the surface");
}

/// Surfaces ride bincode (the cache blob) — the projection must round-trip.
#[test]
fn surface_serde_roundtrip() {
    let s0 = surface(
        "package Acme::W;\nuse parent 'Acme::Base';\nsub area { my ($s,$w)=@_; return $w*2 }\n1;\n",
    );
    let bin = bincode::serialize(&s0).unwrap();
    let back: Surface = bincode::deserialize(&bin).unwrap();
    assert_eq!(s0, back);
}

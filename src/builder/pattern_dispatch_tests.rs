//! Tests for the query-declared plugin capture spike
//! (`src/builder/pattern_dispatch.rs`, design in
//! `docs/prompt-plugin-queries.md`). A child of the `tests` module so
//! it shares `build_fa` and the bundled registry.

use super::*;
use crate::plugin::{
    EmitAction, FrameworkPlugin, MatchContext, PatternSpec, PluginRegistry, Trigger,
};
use std::sync::Arc;

/// Every bundled plugin pattern's `expect` snippets must hold against
/// the real grammar. This is the ring-2 verification story from the
/// design doc, live: a pattern edit that silently stops matching (the
/// field-queryability trap) fails HERE, not in production.
#[test]
fn bundled_pattern_expects_hold() {
    let reg = crate::plugin::default_plugin_registry();
    let mut checked = 0usize;
    for p in reg.all() {
        for spec in p.patterns() {
            assert!(
                !spec.expect.is_empty(),
                "plugin `{}` pattern `{}` has no expect snippets — every \
                 pattern must ship self-verification",
                p.id(),
                spec.name
            );
            crate::builder::pattern_dispatch::verify_pattern_expects(spec)
                .unwrap_or_else(|e| panic!("plugin `{}`: {}", p.id(), e));
            checked += 1;
        }
    }
    assert!(
        checked >= 1,
        "at least mojo-events declares patterns; the loop must have run"
    );
}

// ---- Fixed-point gating ----
//
// A pattern emission can change trigger inputs (PackageParent →
// ClassIsa). The dispatch loop must re-evaluate gates and dispatch
// newly-applicable plugins' matches in a later round.

/// Always-on plugin whose pattern emission makes the enclosing package
/// isa `Widget::Base`.
struct BridgePlugin {
    triggers: Vec<Trigger>,
    patterns: Vec<PatternSpec>,
}

impl BridgePlugin {
    fn new() -> Self {
        Self {
            triggers: vec![Trigger::Always],
            patterns: vec![PatternSpec {
                name: "make".into(),
                language: "perl".into(),
                query: r#"
                    (method_call_expression
                      method: (_) @verb (#eq? @verb "make_widget")
                    ) @call
                "#
                .into(),
                projections: Default::default(),
                expect: vec![],
            }],
        }
    }
}

impl FrameworkPlugin for BridgePlugin {
    fn id(&self) -> &str {
        "test-bridge"
    }
    fn triggers(&self) -> &[Trigger] {
        &self.triggers
    }
    fn patterns(&self) -> &[PatternSpec] {
        &self.patterns
    }
    fn on_match(&self, _pattern: &str, m: &MatchContext) -> Vec<EmitAction> {
        vec![EmitAction::PackageParent {
            package: m.package.clone().unwrap_or_default(),
            parent: "Widget::Base".into(),
        }]
    }
}

/// Gated plugin: fires only for packages isa `Widget::Base` — which
/// only becomes true through BridgePlugin's round-1 emission.
struct WidgetPlugin {
    triggers: Vec<Trigger>,
    patterns: Vec<PatternSpec>,
}

impl WidgetPlugin {
    fn new() -> Self {
        Self {
            triggers: vec![Trigger::ClassIsa("Widget::Base".into())],
            patterns: vec![PatternSpec {
                name: "finish".into(),
                language: "perl".into(),
                query: r#"
                    (method_call_expression
                      method: (_) @verb (#eq? @verb "finish_widget")
                    ) @call
                "#
                .into(),
                projections: Default::default(),
                expect: vec![],
            }],
        }
    }
}

impl FrameworkPlugin for WidgetPlugin {
    fn id(&self) -> &str {
        "test-widget"
    }
    fn triggers(&self) -> &[Trigger] {
        &self.triggers
    }
    fn patterns(&self) -> &[PatternSpec] {
        &self.patterns
    }
    fn on_match(&self, _pattern: &str, m: &MatchContext) -> Vec<EmitAction> {
        vec![EmitAction::Method {
            name: "made".into(),
            span: m.span,
            selection_span: m.span,
            params: vec![],
            is_method: true,
            return_type: None,
            doc: None,
            on_class: None,
            display: None,
            hide_in_outline: false,
            opaque_return: false,
            outline_label: None,
            return_via_edge: None,
            attr: None,
        }]
    }
}

#[test]
fn pattern_dispatch_reaches_fixed_point_over_gating() {
    let mut reg = PluginRegistry::new();
    reg.register(Box::new(BridgePlugin::new()));
    reg.register(Box::new(WidgetPlugin::new()));
    let source = "package P;\n$w->make_widget;\n$w->finish_widget;\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&ts_parser_perl::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let fa = crate::builder::build_with_plugins(&tree, source.as_bytes(), Arc::new(reg));

    // Round 1: bridge (Always) fires, minting the parent edge.
    assert!(
        fa.package_parents
            .get("P")
            .is_some_and(|ps| ps.iter().any(|p| p == "Widget::Base")),
        "bridge plugin's PackageParent emission must land; got {:?}",
        fa.package_parents
    );
    // Round 2: widget's ClassIsa gate is now true, so its match — same
    // tree, same round-1 query pass — dispatches on re-evaluation.
    assert!(
        fa.symbols.iter().any(|s| s.name == "made"
            && matches!(&s.namespace, crate::file_analysis::Namespace::Framework { id } if id == "test-widget")),
        "widget plugin gated on the bridged parent must dispatch in a later round; symbols: {:?}",
        fa.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

/// Trigger gating is per MATCH SITE package, not per file: the same
/// verb in a non-firing package stays silent.
#[test]
fn pattern_dispatch_gates_per_package() {
    let mut reg = PluginRegistry::new();
    reg.register(Box::new(WidgetPlugin::new()));
    let source = "package A;\nuse parent 'Widget::Base';\n$w->finish_widget;\npackage B;\n$w->finish_widget;\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&ts_parser_perl::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let fa = crate::builder::build_with_plugins(&tree, source.as_bytes(), Arc::new(reg));

    let made: Vec<_> = fa
        .symbols
        .iter()
        .filter(|s| s.name == "made")
        .collect();
    assert_eq!(
        made.len(),
        1,
        "only package A (isa Widget::Base) fires the gated pattern; got {:?}",
        made
    );
    assert_eq!(
        made[0].package.as_deref(),
        Some("A"),
        "the emission lands in the firing package"
    );
}

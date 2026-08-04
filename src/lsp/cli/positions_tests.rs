use super::*;

#[test]
fn zero_based_byte_renders_engine_native() {
    // row/byte passed straight through; source line ignored.
    assert_eq!(CoordFmt::ZeroBasedByte.render(4, 30, Some("anything")), (4, 30));
    // No source line still yields the raw byte column.
    assert_eq!(CoordFmt::ZeroBasedByte.render(0, 7, None), (0, 7));
}

#[test]
fn editor_one_based_char_converts_bytes_on_multibyte_line() {
    // `my $msg = "héllo wörld→"; greet();` — the `greet` call starts at byte
    // 30 (é/ö are 2 bytes, → is 3) but character column 26 (0-based) → 27
    // (1-based). A byte renderer would over-count to 31.
    let line = "my $msg = \"héllo wörld→\"; greet();";
    assert_eq!(
        CoordFmt::EditorOneBasedChar.render(4, 30, Some(line)),
        (5, 27),
        "byte 30 on the multibyte line is 1-based char col 27"
    );
    // Fallback (no source) uses the byte column directly, 1-based.
    assert_eq!(CoordFmt::EditorOneBasedChar.render(4, 30, None), (5, 31));
}

#[test]
fn editor_input_round_trips_to_internal_and_back() {
    // Single-line source: editor line 1, char col 27 → internal row 0, byte
    // 30 (the `g` of greet, after é/ö/→ — char index 26 but byte 30).
    let source = "my $msg = \"héllo wörld→\"; greet();";
    let p = editor_to_internal_point(Some(source), 1, 27);
    assert_eq!((p.row, p.column), (0, 30));
    // Rendering that internal point back in editor dialect returns 1:27.
    let line0 = source.lines().next().unwrap();
    assert_eq!(CoordFmt::EditorOneBasedChar.render(p.row, p.column, Some(line0)), (1, 27));
}

#[test]
fn split_at_spec_takes_last_two_colon_fields() {
    assert_eq!(
        split_at_spec("absl/mutex.h:163:48"),
        Some(("absl/mutex.h".to_string(), 163, 48))
    );
    // A path with no colons still needs both line and col.
    assert_eq!(split_at_spec("foo.pm:12"), None);
    assert_eq!(split_at_spec("foo.pm"), None);
    // Extra colons (drive prefix) fold into the file part.
    assert_eq!(
        split_at_spec("C:/src/x.h:9:3"),
        Some(("C:/src/x.h".to_string(), 9, 3))
    );
}

#[test]
fn token_at_byte_finds_word_and_flags_whitespace() {
    let line = "class ABSL_LOCKABLE Mutex {";
    // On the `M` of Mutex (byte 20).
    assert_eq!(token_at_byte(line, 20).as_deref(), Some("Mutex"));
    // One past the end of `Mutex` (the space) still reports it.
    assert_eq!(token_at_byte(line, 25).as_deref(), Some("Mutex"));
    // On the `{` — punctuation, no word.
    assert_eq!(token_at_byte(line, 26), None);
    // Multibyte: on `greet` after a unicode prefix.
    let uni = "my $msg = \"héllo wörld→\"; greet();";
    assert_eq!(token_at_byte(uni, 30).as_deref(), Some("greet"));
}

#[test]
fn parse_cursor_target_picks_dialect_from_form() {
    // Positional → engine dialect.
    let pos = parse_cursor_target(&[
        "f.pm".to_string(), "2".to_string(), "4".to_string(),
    ], ".")
    .unwrap();
    assert_eq!(pos.fmt, CoordFmt::ZeroBasedByte);
    assert_eq!((pos.point.row, pos.point.column), (2, 4));
    // `--at` (missing file on disk) → editor dialect, char col used as byte.
    let at = parse_cursor_target(&[
        "--at".to_string(), "does/not/exist.pm:6:1".to_string(),
    ], ".")
    .unwrap();
    assert_eq!(at.fmt, CoordFmt::EditorOneBasedChar);
    assert_eq!((at.point.row, at.point.column), (5, 0));
    // Malformed → None.
    assert!(parse_cursor_target(&["only-one".to_string()], ".").is_none());
}

#[test]
fn resolve_cursor_file_prefers_cwd_then_root() {
    let dir = std::env::temp_dir().join(format!("perl-lsp-rcf-{}", std::process::id()));
    let sub = dir.join("lib");
    std::fs::create_dir_all(&sub).unwrap();
    let rel = "lib/Thing.pm";
    let abs = dir.join(rel);
    std::fs::write(&abs, "package Thing;\n1;\n").unwrap();

    // Root-relative path that does NOT exist against CWD resolves via <root>.
    let resolved = resolve_cursor_file(rel, dir.to_str().unwrap());
    assert!(std::path::Path::new(&resolved).exists(), "root fallback failed: {}", resolved);

    // An absolute/CWD-existing path is kept verbatim (root not consulted).
    let kept = resolve_cursor_file(abs.to_str().unwrap(), "/nonexistent-root");
    assert_eq!(kept, abs.to_str().unwrap());

    // Neither exists → original returned unchanged for an honest downstream miss.
    let missing = resolve_cursor_file("no/such.pm", dir.to_str().unwrap());
    assert_eq!(missing, "no/such.pm");

    std::fs::remove_dir_all(&dir).ok();
}

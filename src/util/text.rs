//! Byte-bounded string trimming that respects UTF-8.

/// The longest prefix of `s` that fits in `max_bytes` without splitting a
/// character.
///
/// Every caller that caps a string for display wants this, not `&s[..max]` —
/// the naive slice panics whenever the cap lands inside a multibyte char, and
/// a caller that catches the panic per-file silently loses that file's whole
/// analysis. Russian POD in the CPAN corpus is what found it.
pub fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    &s[..cut]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorter_than_cap_is_untouched() {
        assert_eq!(truncate_on_char_boundary("hello", 2000), "hello");
        assert_eq!(truncate_on_char_boundary("", 10), "");
    }

    #[test]
    fn exact_fit_is_untouched() {
        assert_eq!(truncate_on_char_boundary("abcd", 4), "abcd");
    }

    #[test]
    fn ascii_cuts_exactly_at_the_cap() {
        assert_eq!(truncate_on_char_boundary("abcdef", 4), "abcd");
    }

    /// The panic this function exists to prevent: the cap lands on byte 1 of a
    /// 2-byte char, so the prefix has to give that whole char back.
    #[test]
    fn multibyte_cuts_back_to_the_boundary() {
        let s = "аб"; // Cyrillic, 2 bytes each
        assert_eq!(s.len(), 4);
        assert_eq!(truncate_on_char_boundary(s, 3), "а");
        assert_eq!(truncate_on_char_boundary(s, 2), "а");
        assert_eq!(truncate_on_char_boundary(s, 1), "");
    }

    #[test]
    fn wide_chars_cut_back_across_several_bytes() {
        let s = "日本語"; // 3 bytes each
        assert_eq!(truncate_on_char_boundary(s, 5), "日");
        assert_eq!(truncate_on_char_boundary(s, 8), "日本");
        assert_eq!(truncate_on_char_boundary(s, 0), "");
    }

    /// A cap inside a combining sequence still yields valid UTF-8; we bound
    /// bytes, not graphemes.
    #[test]
    fn emoji_never_yields_invalid_utf8() {
        let s = "👍👍";
        for cap in 0..=s.len() {
            let got = truncate_on_char_boundary(s, cap);
            assert!(s.starts_with(got));
            assert!(got.len() <= cap);
        }
    }
}

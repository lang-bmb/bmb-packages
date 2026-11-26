use rust2bmb::{convert, convert_all};

fn kinds(src: &str) -> Vec<&'static str> {
    convert(src, "u")
        .unwrap()
        .findings
        .iter()
        .map(|f| f.kind)
        .collect()
}

#[test]
fn wraps_in_a_unit_and_strips_what_bmb_does_not_have() {
    let out = convert("#[inline]\n#[must_use]\npub fn f(x: u8) { }\n", "u").unwrap();
    assert!(
        out.text.starts_with("#[summary(\"TODO\")]\nmod u {"),
        "{}",
        out.text
    );
    assert!(out.text.contains("#[effects(TODO)]"), "{}", out.text);
    assert!(out.text.contains("fn f(x: u8) -> ()"), "{}", out.text);
    assert!(!out.text.contains("pub "), "{}", out.text);
    assert!(!out.text.contains("inline"), "{}", out.text);
    assert!(!out.text.contains("must_use"), "{}", out.text);
    assert!(out.findings.is_empty(), "{:?}", out.findings);
    assert_eq!(out.items, 1);
    assert_eq!(out.items_clean, 1);
}

#[test]
fn never_writes_contracts() {
    let out = convert(
        "/// Doubles.\nfn f(x: u64) -> u64 { assert!(x < 10); x * 2 }\n",
        "u",
    )
    .unwrap();
    assert!(!out.text.contains("requires"), "{}", out.text);
    assert!(!out.text.contains("ensures"), "{}", out.text);
    assert!(
        out.text.contains("#[summary(\"Doubles.\")]"),
        "{}",
        out.text
    );
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].kind, "macro invocation");
}

#[test]
fn integer_match_becomes_an_if_chain() {
    let src =
        "fn v(c: u8) -> u8 { match c { b'a'..=b'f' => c - 87, b'0' | b'1' => 0, _ => 16 } }\n";
    let out = convert(src, "u").unwrap();
    assert!(
        out.text.contains("if c >= b'a' && c <= b'f'"),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("(c == b'0') || (c == b'1')"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("match"), "{}", out.text);
    assert!(out.findings.is_empty(), "{:?}", out.findings);
}

#[test]
fn a_match_with_a_binding_is_reported_not_guessed() {
    let src = "fn v(c: u8) -> u8 { match c { 0 => 1, x => x } }\n";
    let out = convert(src, "u").unwrap();
    assert!(out.text.contains("match c"), "{}", out.text);
    assert_eq!(out.kinds().get("literal pattern"), Some(&1));
}

#[test]
fn reports_carry_the_line() {
    let src = "fn f() -> u8 {\n    let p = (1, 2);\n    0\n}\n";
    let out = convert(src, "u").unwrap();
    let t: Vec<_> = out.findings.iter().map(|f| (f.line, f.kind)).collect();
    assert_eq!(t, vec![(2, "let without a type"), (2, "tuple expression")]);
}

#[test]
fn iterators_closures_and_char_casts_are_reported() {
    let src = "fn f(s: &[u8]) -> u64 { let n: u64 = s.iter().map(|b| *b as u64).sum(); let c: char = s[0] as char; n }\n";
    let k = kinds(src);
    assert!(k.contains(&"iterator method"), "{k:?}");
    assert!(k.contains(&"closure"), "{k:?}");
    assert!(k.contains(&"cast to char"), "{k:?}");
}

#[test]
fn as_ref_parameters_become_slices() {
    let src = "pub fn f<T: AsRef<[u8]>>(data: T) -> usize { let data = data.as_ref(); data.len() }\npub fn g<T: AsRef<[u8]>>(d: T) -> usize { d.as_ref().len() }\n";
    let out = convert(src, "u").unwrap();
    assert!(
        out.text.contains("fn f(data: &[u8]) -> usize"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("as_ref"), "{}", out.text);
    assert!(out.text.contains("d.len()"), "{}", out.text);
    assert!(out.findings.is_empty(), "{:?}", out.findings);
}

#[test]
fn named_variant_fields_become_positional_across_files() {
    let error = "#[derive(Debug)]\npub enum E { Bad { c: char, index: usize }, Odd }\n";
    let lib = "use crate::error::E;\nfn f(c: char, i: usize) -> E { E::Bad { index: i, c } }\nfn g(e: E) -> usize { match e { E::Bad { c, index } => index, E::Odd => 0 } }\n";
    let outs = convert_all(&[(lib, "lib"), (error, "error")]).unwrap();
    assert!(
        outs[1].text.contains("Bad(char, usize)"),
        "{}",
        outs[1].text
    );
    assert_eq!(outs[1].kinds().get("derive"), Some(&1));
    assert!(
        outs[0].text.contains("error::E::Bad(c, i)"),
        "{}",
        outs[0].text
    );
    assert!(
        outs[0].text.contains("error::E::Bad(c, index) => index"),
        "{}",
        outs[0].text
    );
    assert!(outs[0].text.contains("-> error::E"), "{}", outs[0].text);
}

#[test]
fn items_without_a_counterpart_are_dropped_and_reported() {
    let src = "use core::iter;\nmacro_rules! m { () => {} }\nm!();\nimpl core::fmt::Display for X { }\n#[cfg(test)]\nmod test { }\nmod other;\nstruct X { a: u8 }\n";
    let out = convert(src, "u").unwrap();
    assert_eq!(out.items, 7);
    assert_eq!(out.items_emitted, 1);
    let k = out.kinds();
    for kind in [
        "use",
        "macro definition",
        "macro invocation",
        "impl of a standard-library trait",
        "test module",
        "module declaration",
    ] {
        assert_eq!(k.get(kind), Some(&1), "{kind}: {k:?}");
    }
}

#[test]
fn byte_string_constants_become_arrays() {
    let out = convert("const T: &[u8; 3] = b\"abc\";\n", "u").unwrap();
    assert!(
        out.text.contains("const T: [u8; 3] = [97, 98, 99];"),
        "{}",
        out.text
    );
    assert!(out.findings.is_empty(), "{:?}", out.findings);
}

#[test]
fn literal_suffixes_are_dropped() {
    let out = convert("fn f() -> [u8; 4] { [0_u8; 4] }\n", "u").unwrap();
    assert!(out.text.contains("[0; 4]"), "{}", out.text);
}

#[test]
fn let_types_come_from_initializers_that_name_them() {
    let src = "struct P { x: u64 }\nimpl P { fn new() -> P { P { x: 0 } } fn get(&self) -> u64 { let a = self.x; let b = [0u8; 8]; let c = u64::from_le_bytes(b); let d = a.wrapping_add(c); let n = b.len(); let e = b[0]; let p = P::new(); let q = Self::new(); let f = (d >> 3) as u32; let g = a ^ 1; let mut i = 0; d } }\n";
    let out = convert(src, "u").unwrap();
    for t in [
        "let a: u64",
        "let b: [u8; 8]",
        "let c: u64",
        "let d: u64",
        "let n: usize",
        "let e: u8",
        "let p: P",
        "let q: P",
        "let f: u32",
        "let g: u64",
    ] {
        assert!(out.text.contains(t), "{t}: {}", out.text);
    }
    let lets: Vec<_> = out
        .findings
        .iter()
        .filter(|f| f.kind == "let without a type")
        .collect();
    assert_eq!(lets.len(), 1, "{:?}", out.findings);
    assert!(lets[0].reason.contains("let mut i"), "{:?}", lets);
}

#[test]
fn unsafe_and_paths_from_other_crates_are_reported_where_they_occur() {
    let src = "use core::mem;\nuse digest::Digest;\nunsafe fn f(x: &[u8]) -> usize { mem::size_of::<u64>() + x.len() }\nfn g<D: Digest>() -> usize { unsafe { f(&[]) } }\n";
    let k = convert(src, "u").unwrap().kinds();
    assert_eq!(k.get("unsafe"), Some(&2), "{k:?}");
    assert_eq!(k.get("path from another crate"), Some(&2), "{k:?}");
    assert_eq!(k.get("use"), Some(&2), "{k:?}");
}

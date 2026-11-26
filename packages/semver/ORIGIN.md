# Origin of the `semver` package

## Source

- Crate: `semver` 1.0.28 (crates.io), "Parser and evaluator for Cargo's flavor of Semantic Versioning".
- Repository: <https://github.com/dtolnay/semver>, commit `7625c7aa3f0e8ba21e099d1765bcebcb72aa8816` (from the crate's `.cargo_vcs_info.json`).
- Files read: `src/lib.rs`, `src/parse.rs`, `src/eval.rs`, `src/display.rs`, `src/identifier.rs`, `src/impls.rs`, `src/error.rs`, `tests/test_version.rs`, `tests/test_version_req.rs`, `tests/test_identifier.rs`.
- Licence of the origin: `MIT OR Apache-2.0` (its `Cargo.toml`). This port uses it under the MIT licence. The origin's licence texts carry no copyright line; they are kept unchanged in [`LICENSE-MIT-origin`](LICENSE-MIT-origin) and [`LICENSE-APACHE-origin`](LICENSE-APACHE-origin). The port itself is under the repository's MIT licence.

## What was ported

| Origin (Rust) | Port (BMB, `src/semver.bmb`) |
| --- | --- |
| `Version { major, minor, patch, pre, build }`, `Version::parse`, `Display` | `Version` (pre-release and build metadata kept as their text, `Vec<u8>`), `parse_version`, `print_version` |
| `Ord for Version` (derived), `Version::cmp_precedence` | `cmp`, `cmp_precedence` (-1, 0 or 1) |
| `Ord for Prerelease`, `Ord for BuildMetadata` | `compare_pre`, `compare_build` |
| `Prerelease::new`, `BuildMetadata::new` | `parse_prerelease`, `parse_build` |
| `Comparator` (operators `=`, `>`, `>=`, `<`, `<=`, `~`, `^` and wildcards), `Comparator::parse`, `matches`, `Display` | `Comparator`, `parse_comparator`, `comparator_matches`, `print_comparator` |
| `VersionReq` (comma-separated comparators, at most 32, or a lone `*`/`x`/`X`), `parse`, `matches`, `Display` | `VersionReq`, `parse_req`, `req_matches`, `print_req` |
| `Error` and its `Display` | `Error` and `message` |

All of `Version` and all of `VersionReq` and `Comparator` are ported. Nothing in the scope was stopped short.

**Build metadata and the SemVer specification.** SemVer 2.0.0 §10 says that build metadata must be ignored when determining precedence. The origin follows it in `cmp_precedence`, but its derived `Ord` (and `Eq`) break ties on build metadata, with numeric identifiers compared by value and then by length, so `1.2.3+23 < 1.2.3+42` and `1.2.3+1 != 1.2.3+001`. The port keeps both: `precedence` is §11 exactly, and `version_order` is the origin's `Ord`.

## Contracts

Stated and proved; `bmb verify` proves the 58 definitions with no `assume`, no `#[external_body]` and no unproved termination:

- **The order of pre-releases is the specification's §11.4**, written as spec functions:
  - `seg_end`, `all_digits` and `lex` describe one identifier and the byte order.
  - `idcmp` compares two identifiers: numeric ones by length then digits, numeric below alphanumeric, alphanumeric ones by bytes.
  - `pcmp` goes identifier by identifier; a shorter list is lower.
  - `pre_order` adds §11.3: no pre-release is higher.
  - `compare_pre` is proved equal to `pre_order`. Proved lemmas: `pre_order` is antisymmetric (`pre_order(a, b) == -pre_order(b, a)`) and reflexive.
- **Version precedence** (`precedence`, §11) and the origin's total order (`version_order`, with `build_order`) are spec functions. `cmp_precedence` and `cmp` are proved equal to them, and precedence is proved antisymmetric and reflexive.
- **Matching** is stated exactly for every operator, as the spec functions `m_exact` (`=` and wildcards), `m_greater`, `m_less`, `m_tilde`, `m_caret` and `m_op` (which adds `>=` and `<=`), plus the pre-release rule `pre_ok`.
  - `comparator_matches` is proved equal to `matches_comparator`.
  - `req_matches` is proved equal to `matches_req`: every comparator's operator matches, and a pre-release only when some comparator allows it (a `forall` and an `exists` over the comparators).
- **Reading a number** (`numeric`): on success, the longest run of digits, with no leading zero, and its decimal value.

Wanted and not stated:

- **Transitivity of the order** of pre-releases. The proof needs a three-way induction over the identifier lists and a lemma that two identifiers comparing equal have the same bytes. It was not attempted in the time of this port. Antisymmetry and reflexivity are proved. Totality holds by construction, since the result is always -1, 0 or 1.
- **`parse(print(v)) == Ok(v)` and `print(parse(t)) == t`.** The first needs a predicate for the versions that are well formed (the grammar of §9 and §10). The second needs to know that the decimal text `std::bytes::push_u64` writes reads back. Its contract only says how much the buffer grows (`grew_by`); `std::num::push_u64`, on `String`, has a round-trip lemma but works on text. Neither statement is made. The tests check `print(parse(t)) == t` on 200 pseudo-random versions and the origin's cases.
- **Exact contracts of the parsers** beyond `numeric`: the error each malformed input gives is tested, not stated.

**Proof cost** (bmb-new, Z3 4.15.4): 58 definitions (and 8 of `std::bytes`), 485 obligations, total rlimit 2,296,783, the largest single query 45,495, in about 4 s.

- The largest definitions are `comparator_at` (461,992), `req_matches` (370,604) and `numeric` (164,212).
- `bmb stability` over seeds 0 to 4 finds no definition whose verdict depends on the seed; the largest spread is 1.17x.

## What was changed, and why

- **The identifier type** (`identifier.rs`, a small-string-optimized pointer with `unsafe` code) is a `Vec<u8>`.
- **Errors keep the character as its Unicode scalar value (`u32`).** `message` writes it as the origin's `QuotedChar` does (`'\0'` for NUL, else Rust's `Debug` of the char) for ASCII. Characters above U+007F are written as they are, which is Rust's `Debug` for printable ones. Rust escapes non-printable characters above U+007F (U+00AD, for one); that is not ported.
- **The comparator list is built in a loop**, not by the origin's recursion with `unsafe` writes into a vector with no length set. The limit of 32 comparators and the error order are the origin's.
- **Strings are bytes.** The functions take `&[u8]`; a caller with text passes `s.as_bytes()`. Printing returns `Vec<u8>`.
- **Orders are `i8`** (-1, 0, 1): BMB has no `Ordering` or `Ord` for structs.

## What was left out

- serde support; `Hash`; `Default` (`VersionReq::default()` is `parse_req(b"*")`); `FromIterator`.
- `Display` padding and alignment (`test_align`: `{:20}`, `{:*^20}`).
- The 5.6-megabyte pre-release of `test_identifier::test_new` (280 digits repeated 20,000 times); the 280 shorter ones are tested.
- The `node` comparison tests (behind `cfg(test_node_semver)`).

## Tests

The test suite is declared in `package.json` and run by `gotgan test`: gotgan builds `tests/driver.bmb` with `src/semver.bmb` (`bmb build`, which proves everything first) and runs each case of the two case files (gotgan's format `ARGS|STDIN|STDOUT`).

- **`tests/cases.txt`, 589 cases**, transcribed by a script from the origin's three test files:
  - every error message;
  - every parsed version with its fields;
  - every printed form;
  - every comparison whose result the test states (`assert_eq!`, `<`, `>`), including the §11 example chain of `test_spec_order`;
  - every `assert_match_all` and `assert_match_none` (213 matches);
  - every requirement and comparator with its printed form or error;
  - the 280 pre-releases of `test_identifier`.
- **`tests/reference.txt`, 3,440 cases**, with the expected values computed by the origin crate itself (`tests/reference/`, a Cargo program depending on `semver = "=1.0.28"`):
  - the comparisons the origin's tests do not state exactly (`assert_ne!`, `<=`, `>=`, negations), as `cmp` and `precedence`;
  - the equal spellings of requirements (`x`, `X`, `1.x`, `1.*.*` ..) and the requirements of `test_leading_digit_in_pre_and_build`, which the origin only parses;
  - 200 pseudo-random versions printed back, 400 pairs of them compared both ways;
  - 300 pseudo-random requirements printed back and matched against versions (`matches`, and `matches_comparator` for one comparator);
  - 300 pieces of malformed text read as a version, a requirement and a comparator.

Result: 4,029 passed, 0 failed (`gotgan test`, about 9 s).

Not exercised: requirements of more than a few comparators other than the 33-comparator error case, and non-ASCII characters other than `é` and `ÿ` in error messages.

## The rewriting tool on this crate

Run as `rust2bmb src/lib.rs=semver src/parse.rs src/eval.rs src/display.rs src/identifier.rs src/impls.rs src/error.rs`. The numbers are in [`tools/rust2bmb/README.md`](../../tools/rust2bmb/README.md#measured-on-semver-1028).

- 1,222 code lines and 118 items: 59 written (13 with nothing reported), 59 dropped.
- 420 constructs reported and 185 rewrites applied.
- The raw output does not pass `bmb check`: 387 diagnostics.
- `src/semver.bmb` was written by hand. Of its 1,299 non-blank lines, 236 are identical to a line of the tool's output. 208 of those are braces, `} else {` or `Ok(())`, and of the other 28 most are `return false;`, `};` and lines of the comparator reader.
- The 1,299 lines:

  | kind | lines |
  | --- | --- |
  | `spec fn` | 176 |
  | contract clauses | 88 |
  | `proof fn` headers | 9 |
  | `#[summary]` | 68 |
  | `#[effects]` | 42 |
  | `#[uses]` | 32 |
  | code | 884 |

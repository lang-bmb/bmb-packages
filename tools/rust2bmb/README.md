# rust2bmb

Rewrites Rust source into the shape of BMB source, and reports every construct it does not rewrite.

BMB's surface is a subset of Rust with mandatory contracts. Porting a Rust crate is therefore mostly rewriting what falls outside the subset. This tool does the rewrites that are mechanical and, for everything else, prints a report line with the position, the kind of construct and the reason. It does not guess: a construct it cannot rewrite is left as written and reported. It never writes contracts, and it does not choose effects: every function gets `#[effects(TODO)]`, which the compiler rejects until a person (or an agent) writes the real set.

The tool reads syntax only (`syn`). What needs type information to judge, such as which method a call resolves to, is passed through, and `bmb check` is the judge for it.

## Use

```
cargo build --release
target/release/rust2bmb [-o DIR] FILE.rs[=UNIT]..
```

Each file becomes one unit, named `UNIT` or after the file. The units are written to `DIR/UNIT.bmb`, or one after another to standard output. Standard error gets one line per reported construct, `file:line:col: kind: reason`, then per file a summary and the counts of the reported kinds and of the rewrites. Files given together are one crate: an enum declared in one file is rewritten where another uses it, and `use crate::m::Name;` makes `Name` the full path `m::Name`.

## What it rewrites

- wraps the file in `#[summary("..")] mod UNIT { .. }`; the summary is the first paragraph of the file's `//!` comment, or `TODO`;
- drops visibility (`pub`, `pub(crate)`), `#[inline]`, `#[must_use]`, `#[allow]`, `#[cfg]`, `#[cfg_attr]` and inner attributes (it assumes the crate's default features: a `cfg`-gated item is kept);
- turns the first paragraph of a `///` comment into `#[summary("..")]` (on functions, types, traits and constants; on variants and fields, where BMB has no summary, the comment is dropped);
- places `#[effects(TODO)]` on every function and writes a missing return type as `-> ()`;
- writes receivers as `self: T`, `self: &T`, `self: &mut T`;
- turns a type parameter bounded only by `AsRef<[u8]>` into `&[u8]`, drops `let x = x.as_ref();` and writes `x.as_ref()` as `x`;
- turns `const X: &[u8; N] = b"..";` into `const X: [u8; N] = [..];`;
- turns enum variants with named fields into positional ones, and their constructions and patterns into positional ones in declaration order;
- turns a `match` on an integer whose arms are literals, ranges, `|` of those and a final `_` into an `if` chain;
- writes field init shorthand as `name: name`, drops integer literal suffixes, and replaces `use crate::m::Name;` by full paths; a `use` of a type BMB has built in (`String`, `Vec`, `Box`, `Option`, `Result`) is dropped;
- writes the type of a `let` when its initializer names it (`src/infer.rs`): a suffixed, byte, `bool` or `char` literal, a cast, an array or repeat of those, `.len()`, `wrapping_*`/`rotate_*`/`to_le_bytes` of a typed value, an integer type's `from_le_bytes` or `MAX`, a struct literal, a call of a function of the crate with a written return type (`f`, `T::f`, `Self::f`), a field of a struct of the crate, an element of an array or slice, arithmetic with a typed left operand, a comparison. Any other `let` stays reported.

Items with no BMB counterpart at all (`use` of other paths, `extern crate`, `mod m;`, `#[cfg(test)]` modules, macro definitions and item-level macro calls, impls of standard-library traits) are dropped and reported. Every other item is written, with what is inside it reported. A name brought in by a `use` of another crate (the Rust standard library included) is reported again at every place the written code uses it (`path from another crate`), and so are `unsafe` functions and blocks.

### Added after the first measurement

What `siphasher` and `hmac` showed was mechanical and not yet done, and what was added for it:

1. **`let` types from the initializer.** `let` without a type was 72 of siphasher's reports. The tool now writes the type wherever the initializer settles it (the list above), 49 of the 72 in siphasher, 2 of 3 in hex. What is left needs a type checker: `let mut i = 0;` takes its type from later uses.
2. **Uses of names from other crates are reported where they occur.** The `use` report used to say so without doing it, so `PhantomData`, `mem::size_of`, `cmp::min` in siphasher and every `digest` trait and type in hmac (`D: Digest + BlockSizeUser`, `Block<D>`, `Output<Self>`) passed silently. They are now 17 reports in siphasher, 157 in hmac and 5 in hex. A single-name `use` (a re-export of an item of the same file, such as `pub(crate) use load_int_le;`) is reported as such, not as another crate.
3. **`unsafe` is reported** (11 in siphasher: `unsafe fn u8to64_le` and the blocks around the pointer loads). It passed silently before.

## Measured on hex 0.4.3

Run on the origin of `packages/hex` (`rust2bmb src/lib.rs=hex src/error.rs`, the crate unpacked from crates.io):

| | `src/lib.rs` | `src/error.rs` | together |
| --- | --- | --- | --- |
| code lines (not blank, not `//`) | 310 (191 without `mod test`) | 41 (20 without `mod tests`) | 351 (211) |
| items | 32 | 5 | 37 |
| written / with nothing reported / dropped | 17 / 5 / 15 | 1 / 0 / 4 | 18 / 5 / 19 |
| reported constructs | 61 | 5 | 66 |
| rewrites applied | 71 | 6 | 77 |
| raw output passes `bmb check` | no: 30 diagnostics (12 E0103 parse errors, 9 E0208 lifetimes, 9 E0703 items outside a `mod` after the parser lost the `mod`) | yes, on its own (the `#[derive]` was dropped and reported) | no |

The reported constructs, counted (this is the main result: it shows which Rust idioms of this crate fall outside the BMB surface):

| kind | count |
| --- | --- |
| iterator method (`iter`, `iter_mut`, `chunks`, `enumerate`, `map`, `zip`, `skip`, `step_by`, `collect`) | 14 |
| bound on a standard-library trait (`FromIterator<char>`, `AsRef<[u8]>` on an impl) | 6 |
| tuple pattern | 5 |
| impl of a standard-library trait (`Iterator`, `ExactSizeIterator`, `Display`, `Error`) | 4 |
| lifetime (in a reference type) | 4 |
| let without a type | 3 |
| macro invocation (`from_hex_array_impl!` at item level) | 3 |
| associated type | 2 |
| for over an iterator | 2 |
| for pattern (a tuple as the loop variable) | 2 |
| lifetime parameter | 2 |
| missing method (`as_ref` on `self`) | 2 |
| module declaration (`mod error;`, `mod serde;`) | 2 |
| test module (`#[cfg(test)] mod test`) | 2 |
| use (`use core::iter`, `use core::fmt`) | 2 |
| blanket impl (`impl<T: AsRef<[u8]>> ToHex for T`) | 1 |
| cast to char (`c as char`) | 1 |
| closure | 1 |
| derive | 1 |
| extern crate | 1 |
| impl Trait type (`-> impl Iterator<Item = (usize, usize)>`) | 1 |
| macro definition (`macro_rules!`) | 1 |
| reference outside a parameter (`&'static [u8; 16]` in a field) | 1 |
| supertrait (`: Sized`) | 1 |
| tuple expression | 1 |
| tuple type | 1 |

Grouped by idiom: iterators and closures 18 (iterator methods, `for` over an iterator, the closure, the `impl Iterator` return), standard-library traits 17 (bounds, impls, associated types, supertrait, blanket impl, derive, `as_ref`), tuples 9, lifetimes and stored references 7, the module system 7 (`mod`, `use`, `extern crate`, test modules), macros 4, type inference 3 (`let` without a type: the tool has no type checker), `as char` 1.

The rewrites applied (77): 16 `#[effects(TODO)]`, 11 doc comments to `#[summary]`, 3 doc comments on variants dropped, 10 attributes dropped, 8 visibility, 7 `AsRef<[u8]>` parameters, 5 `as_ref` uses (2 `let`, 3 calls), 4 receivers, 4 `use crate::..` to full paths, 2 built-in `use`s, 2 byte-string constants, 1 named-field enum, 1 construction of a named-field variant, 1 integer `match` to an `if` chain, 1 field init shorthand, 1 inner attributes.

How much of the final `packages/hex/src/hex.bmb` came from the tool: it was written by hand from the origin, not by editing the tool's output, and the overlap was measured afterwards by matching trimmed lines. Of its 339 non-blank lines, 43 are identical to a line of the tool's output; 33 of those are braces, `} else {` or `Ok(())`, and 10 are substantive (the `mod` line, three length checks, and the six lines of `val`'s `if` chain, which the tool made from the origin's `match`). The other 296 were written by hand: 106 lines of `spec fn`, 20 contract clauses, 8 lines of proof, 40 `#[summary]`, 13 `#[effects]`, 7 `#[uses]`, and the code that replaces iterators and tuples. Line matching undercounts a little: the tool's printer and BMB's normal form lay out some expressions differently.

Re-measured with the current tool (after the additions above): 69 reports (64 + 5) and 79 rewrites. `let` types were written for 2 `let`s (`let high = table[..]` and `let low`); the remaining `let (high, low)` is a tuple. The 5 `path from another crate` reports are `iter::FromIterator`, written five times. The raw output still does not check: the same 30 diagnostics.

## Measured on siphasher 1.0.4

Run as `rust2bmb src/lib.rs=siphasher src/sip.rs src/sip128.rs src/common.rs` (the test files `src/tests.rs`, `src/tests128.rs` are the package's test data, not ported code), with the current tool:

| | `lib.rs` | `sip.rs` | `sip128.rs` | `common.rs` | together |
| --- | --- | --- | --- | --- | --- |
| code lines | 23 | 413 | 496 | 55 | 987 |
| items | 7 | 25 | 37 | 5 | 74 |
| written / with nothing reported / dropped | 0 / 0 / 7 | 18 / 0 / 7 | 25 / 5 / 12 | 1 / 0 / 4 | 44 / 5 / 30 |
| reported constructs | 7 | 61 | 69 | 11 | 148 |
| rewrites | 1 | 255 | 304 | 4 | 564 |
| raw output and `bmb check` (each unit alone) | checks (only `mod` items were left) | 156 diagnostics | 177 | 14 | 347 together |

The 347 diagnostics together: 61 E0103 (parse), 54 E0306, 44 E0602 (`#[effects(TODO)]`), 26 E0404, 19 E0305, 18 E0409, 17 E0201, 14 E0233, 11 E0215, 11 E0304, 10 E0435, 9 E0401, 7 E0402, 7 E0408, 6 each E0202, E0310, E0420, 4 E0237, 3 each E0301, E0308, 2 each E0205, E0210, E0424, E0434, E0506, 1 E0601.

Reported constructs, counted:

| kind | count |
| --- | --- |
| macro invocation (`compress!` 20, `load_int_le!` 6, `debug_assert!` 3, `debug_assert_eq!` 1) | 30 |
| let without a type | 23 |
| path from another crate (`hash::Hasher` 8, `PhantomData` 5, `cmp::min` 2, `mem::size_of` 2) | 17 |
| derive | 15 |
| use | 12 |
| unsafe | 11 |
| tuple type (`keys() -> (u64, u64)`, `as_u64`) | 7 |
| tuple expression | 7 |
| impl of a standard-library trait (`Default` 2, `Clone`, `PartialEq`, `Eq`, `From` 2; the impls of `hash::Hasher` are written and their trait counted as `path from another crate`) | 7 |
| tuple struct (`SipHasher(SipHasher24)`) | 6 |
| parameter pattern (`_: &mut State` in the `Sip` trait) | 4 |
| module declaration | 3 |
| test module, macro definition, inline module (`pub mod prelude`, `reexports`) | 2 each |

Rewrites: 123 `#[effects(TODO)]`, 110 attributes dropped (mostly `#[inline]`), 88 receivers, 64 missing return types, 51 visibility, 51 doc comments, 49 `let` types, 19 literal suffixes, 6 `use crate::..` to full paths, 2 field init shorthands, 1 inner attributes.

Of the 442 non-blank lines of `packages/siphash/src/siphash.bmb` (written by hand), 61 are identical to a line of the tool's output, all braces or `} else {`. The origin's SipRound is the macro `compress!`, which the tool drops. Its word loads are `unsafe` macro calls. The port's state-machine specification and its lemmas have no counterpart line.

## Measured on hmac 0.13.0

Run as `rust2bmb src/lib.rs=hmac src/simple.rs src/simple_reset.rs src/block_api.rs src/utils.rs`, with the current tool:

| | `lib.rs` | `simple.rs` | `simple_reset.rs` | `block_api.rs` | `utils.rs` | together |
| --- | --- | --- | --- | --- | --- | --- |
| code lines | 36 | 54 | 74 | 174 | 21 | 359 |
| items | 13 | 11 | 14 | 28 | 4 | 70 |
| written / with nothing reported / dropped | 2 / 0 / 11 | 7 / 0 / 4 | 9 / 0 / 5 | 21 / 0 / 7 | 3 / 2 / 1 | 42 / 2 / 28 |
| reported constructs | 27 | 49 | 69 | 131 | 9 | 285 |
| rewrites | 3 | 16 | 24 | 41 | 5 | 89 |
| raw output and `bmb check` (each unit alone) | 6 diagnostics | 15 | 18 | 48 | 11 | 98 together |

The 98 diagnostics together: 52 E0305 (unknown types, the `digest` crate's), 27 E0103 (parse), 10 E0263, 4 E0208, 2 E0201, 2 E0304, 1 E0602.

Reported constructs, counted:

| kind | count |
| --- | --- |
| path from another crate (the `digest` crate's traits and types: `Digest`, `BlockSizeUser`, `Block`, `Output`, `KeyInit`, `EagerHash`, ..) | 157 |
| use | 59 |
| let without a type | 21 |
| associated type (`KeySize` 4, `OutputSize` 4, `BlockSize` 2, `BufferKind` 2) | 12 |
| closure | 10 |
| iterator method | 8 |
| impl of a standard-library trait (`fmt::Debug` 4, `Clone` 2) | 6 |
| module declaration | 4 |
| missing method (`expect`) | 4 |
| macro invocation (`digest::buffer_fixed!`) | 2 |
| derive | 2 |

Rewrites: 24 `#[effects(TODO)]`, 13 attributes dropped, 11 receivers, 11 missing return types, 9 `use crate::..` to full paths, 8 field init shorthands, 7 visibility, 4 doc comments, 1 `let` type, 1 inner attributes.

Of the 161 non-blank lines of `packages/hmac/src/hmac.bmb` (written by hand), 30 are identical to a line of the tool's output: 29 braces or `} else {`, and `mod hmac {`. The origin is a generic HMAC over the `digest` crate's traits, and the port is a concrete HMAC-SHA256 over `std::sha256`. Almost nothing of the origin's text carries over, which the 157 `path from another crate` reports measure.

## Measured on semver 1.0.28

Run as `rust2bmb src/lib.rs=semver src/parse.rs src/eval.rs src/display.rs src/identifier.rs src/impls.rs src/error.rs` (`src/serde.rs` is out of scope), with the current tool:

| | lib | parse | eval | display | identifier | impls | error | together |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| code lines | 151 | 341 | 144 | 145 | 202 | 124 | 115 | 1,222 |
| items | 25 | 22 | 10 | 12 | 23 | 16 | 10 | 118 |
| written / with nothing reported / dropped | 11 / 1 / 14 | 17 / 2 / 5 | 9 / 5 / 1 | 2 / 1 / 10 | 15 / 3 / 8 | 2 / 0 / 14 | 3 / 1 / 7 | 59 / 13 / 59 |
| reported constructs | 25 | 171 | 14 | 43 | 135 | 22 | 10 | 420 |
| rewrites | 70 | 42 | 11 | 2 | 51 | 6 | 3 | 185 |
| raw output and `bmb check` (each unit alone) | 50 diagnostics | 157 | 104 | 27 | 62 | 6 | 1 | 387 together |

Reported constructs, counted: let without a type 78, path from another crate 70 (`ptr` 18, `NonNull` 9, `NonZeroUsize` 8, `mem` 8, `str` 6, `FromStr` 5, ..), use 42, unsafe 39 (most in `identifier.rs`), tuple expression 33, tuple pattern 27, impl of a standard-library trait 24, if let 20, reference outside a parameter 16 (`&str` in results and fields), tuple type 7, module declaration 7, derive 7, associated type 7, range pattern 6, macro invocation 5, iterator method 5, missing method 4, associated constant 4, method turbofish 3, let else 3, literal pattern 2, impl Trait type 2, for pattern 2, for over an iterator 2, while let 1, tuple struct 1, match scrutinee 1, extern crate 1, closure 1.

Rewrites: 57 `#[effects(TODO)]`, 42 visibility, 22 `let` types, 17 field init shorthands, 15 receivers, 12 doc comments, 11 `use crate::..` to full paths, 3 built-in `use`s, 2 literal suffixes, 2 doc comments on fields dropped, 1 missing return type, 1 inner attributes.

Of the 1,299 non-blank lines of `packages/semver/src/semver.bmb` (written by hand), 236 are identical to a line of the tool's output. 208 of those are braces, `} else {` or `Ok(())`, and the other 28 are mostly `return false;`, `};` and lines of the comparator reader (`let mut has_wildcard: bool = false;`, `op = Op::Wildcard;`).

## Measured on percent-encoding 2.3.2

Run as `rust2bmb src/lib.rs=percent_encoding src/ascii_set.rs`, with the current tool:

| | `lib.rs` | `ascii_set.rs` | together |
| --- | --- | --- | --- |
| code lines | 303 | 148 | 451 |
| items | 22 | 13 | 35 |
| written / with nothing reported / dropped | 10 / 0 / 12 | 8 / 1 / 5 | 18 / 1 / 17 |
| reported constructs | 73 | 16 | 89 |
| rewrites | 47 | 40 | 87 |
| raw output and `bmb check` | | | no: 102 diagnostics together (23 E0208 lifetimes, 21 E0103, 17 E0101 for the byte table's string continuation lines, ..) |

Reported constructs, counted: path from another crate 26, use 10, let without a type 8, lifetime 7, reference outside a parameter 5, lifetime parameter 5, impl of a standard-library trait 5, iterator method 3, derive 3, unsafe 2, test module 2, macro invocation 2, extern crate 2, associated type 2, type alias 1, module declaration 1, missing method 1, macro definition 1, if let 1, closure 1, associated constant 1.

Rewrites: 18 visibility, 18 `#[effects(TODO)]`, 16 doc comments, 11 receivers, 10 attributes dropped, 6 `let` types, 5 field init shorthands, 2 built-in `use`s, 1 inner attributes.

Of the 412 non-blank lines of `packages/percent_encoding/src/percent_encoding.bmb` (written by hand), 42 are identical to a line of the tool's output, and only the `mod` line is not a brace: the port's set is an array of `bool`s and its encoder a loop, where the origin has bit masks and iterators.

### Fixed while measuring semver and percent-encoding

Reports that were silent or vague, not new rewrites:

- `if let` and `while let` passed without a report. They are now reported (20 in semver, 1 in percent-encoding).
- A type alias was reported as "an item kind BMB does not have"; it is now `type alias`.
- An associated constant in an impl was reported as "only functions are accepted in a BMB impl"; it is now `associated constant`.
- `use core::ptr::{self, NonNull};` recorded the name `self` as coming from another crate, so every `self` in the written code was reported as a path from another crate (13 wrong reports in semver), and the uses of `ptr` were not. It now records `ptr`.

The earlier three packages' numbers are unchanged by these (re-measured).

## Per package

| package | origin code lines | items written / dropped | reports | rewrites | raw output checks | final lines matching the tool's output (substantive) |
| --- | --- | --- | --- | --- | --- | --- |
| hex | 351 | 18 / 19 | 69 | 79 | no (30) | 43 of 339 (10) |
| siphash | 987 | 44 / 30 | 148 | 564 | no (347) | 61 of 442 (0) |
| hmac | 359 | 42 / 28 | 285 | 89 | no (98) | 30 of 161 (1) |
| semver | 1,222 | 59 / 59 | 420 | 185 | no (387) | 236 of 1,299 (28) |
| percent_encoding | 451 | 18 / 17 | 89 | 87 | no (102) | 42 of 412 (1) |

## Checks

```
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
```

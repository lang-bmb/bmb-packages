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
- writes field init shorthand as `name: name`, drops integer literal suffixes, and replaces `use crate::m::Name;` by full paths; a `use` of a type BMB has built in (`String`, `Vec`, `Box`, `Option`, `Result`) is dropped.

Items with no BMB counterpart at all (`use` of other paths, `extern crate`, `mod m;`, `#[cfg(test)]` modules, macro definitions and item-level macro calls, impls of standard-library traits) are dropped and reported. Every other item is written, with what is inside it reported.

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

## Checks

```
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
```

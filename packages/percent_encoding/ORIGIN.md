# Origin of the `percent_encoding` package

## Source

- Crate: `percent-encoding` 2.3.2 (crates.io), "Percent encoding and decoding", part of the rust-url project.
- Repository: <https://github.com/servo/rust-url> (directory `percent_encoding`), commit `91377f48bf35011d042aa5abef9e7f2a0a625aaa` (from the crate's `.cargo_vcs_info.json`, which marks the tree it was packaged from as modified).
- Files read: `src/lib.rs`, `src/ascii_set.rs`.
- Licence of the origin: `MIT OR Apache-2.0` (its `Cargo.toml`). This port uses it under the MIT licence. The origin's notice is `Copyright (c) 2013-2025 The rust-url developers` (its `LICENSE-MIT`). Its two licence texts are kept unchanged in [`LICENSE-MIT-origin`](LICENSE-MIT-origin) and [`LICENSE-APACHE-origin`](LICENSE-APACHE-origin). The port itself is under the repository's MIT licence.
- The package name is `percent_encoding`: gotgan's names are letters, digits and `_`.

## What was ported

| Origin (Rust) | Port (BMB, `src/percent_encoding.bmb`) | Contract |
| --- | --- | --- |
| `AsciiSet`, `AsciiSet::EMPTY`, `CONTROLS`, `NON_ALPHANUMERIC` | the same, as `const`s | (values) |
| `contains`, `should_percent_encode` | the same | exact: `has(s, b)`, `encodes(s, b)` |
| `add`, `remove`, `union`, `complement` (and the operators `+`, `!`) | the four functions | exact, for every byte |
| `percent_encode(bytes, set)` (an iterator of `&str`), `utf8_percent_encode` | `percent_encode(input, s) -> String` (callers with text pass `s.as_bytes()`) | exact, byte by byte (below) |
| `percent_decode(bytes)` (an iterator of `u8`), `percent_decode_str` | `percent_decode(input) -> Vec<u8>` | exact (below) |
| `percent_encode_byte` | `percent_encode` of one byte under a set holding every ASCII byte (`complement(EMPTY)`) | as above |
| (none) | `round_trip(x, s)` | `percent_decode(percent_encode(x, s)) == x` whenever `s` holds `%` |

## Contracts

All of them are stated and proved. `gotgan verify` proves the 19 definitions with no `assume`, no `#[external_body]` and no unproved termination, together with those of the package it depends on.

- **The encoder** (`encoded`):
  - Byte `i` of the input becomes the piece at offset `elen(s, x, i)` of the output, where `elen` adds up the widths of the pieces before it (3 for an encoded byte, 1 otherwise).
  - An encoded byte (not ASCII, or in the set) is `%` and its two upper-case hex digits; any other byte stays.
  - The output is exactly as long as the pieces, every byte of it is ASCII (`ascii`), and its length is between `x.len()` and `3 * x.len()`.
- **The decoder** (`decoded`):
  - The scan the origin does is stated as spec functions: `pos(t, k)` is where output byte `k` starts in the input, `step` is 3 at `%` followed by two hex digits (either case) and 1 otherwise, and `out_at` is the byte written there.
  - The result has one byte per step up to the end of the input, so a `%` not followed by two hex digits stays, as in the origin. The result is no longer than the input.
- **The round trip:** `round_trip(x, s)` requires `s.bits[37]` (`%` is in the set) and ensures the result is `x`, byte for byte. It is proved from the two contracts by the lemmas `rt_step`, `rt_pos`, `rt_byte` and `rt_all`.
  - The precondition is right as it is: without `%` in the set, a literal `%41` in the input is encoded as itself and decoded to `A`.
- **The set operations:** `add`/`remove` state for every ASCII byte whether it is in the result; `union` and `complement` the same.

**Hex digits come from the package `hex`.** `package.json` declares it, `"deps": { "std": [], "packages": { "hex": { "path": "../hex" } } }`, and gotgan passes `hex`'s unit with this one to the compiler. The encoder calls `hex::hex_char(x, true)`, the decoder `hex::value(c)`. The contracts name `hex`'s spec functions directly (`piece_of` says `t[o + 1] == hex::digit(b / 16, true)`; `out_at` and `triple` use `hex::val_of` and `hex::is_hex`). Spec functions stay opaque across the package boundary just as inside a unit. The lemmas `rt_byte` and `rt_step` list `hex::digit`, `hex::is_hex` and `hex::val_of` in `#[uses(..)]`, and `percent_decode` lists `hex::is_hex`. Each of those names is needed: with any one of them removed, that definition fails. `std::num`, which `hex` uses, is declared by `hex`, not here.

**Proof cost** (the compiler of 2026-11-26, Z3 4.15.4): 19 definitions of this package, 161 obligations, total rlimit 1,250,702, the largest single query 58,067. The 14 definitions of `hex` and the 24 of `std::num` are proved with them (`gotgan verify`: `defs` 19, `dep_defs` 14, `std_defs` 24); `bmb verify` of the two units takes about 3 s.

- The largest definitions are `percent_decode` (340,819), `percent_encode` (326,790) and `round_trip` (214,100).
- Calling `hex` cost nothing measurable. When the digit functions were this package's own, the same definitions cost: `push_encoded` 145,153 (now 145,403), `rt_byte` 55,054 (now 55,144), `rt_step` 35,708 (now 35,768). `percent_decode` is unchanged.
- `bmb stability` over seeds 0 to 4 finds no definition whose verdict depends on the seed, here or in `hex`. The largest spread is 7.29x (`round_trip`, 207,491 to 1,512,036).

## What was changed, and why

- **`AsciiSet` is 128 `bool`s, not four `u32` words.** With the origin's bit mask, `has(s, b)` is a shift and a mask. Under a quantifier the prover did not carry such facts across an append to the output, and the same failure appeared with any opaque predicate in that position. An array element works. The set's meaning and every function's behaviour are the same; the memory layout is not.
- **`percent_encode` keeps a `Vec<usize>` of piece offsets while it builds the text.** The fact "piece `i` is at offset `elen(s, x, i)`", with `elen` a recursive spec function, could not be carried across an append. With the offsets in an array (`st[i]`) and the fact `st[i] == elen(s, x, i)` kept apart, it can. BMB has no ghost variables, so this vector exists at run time; it costs 8 bytes per input byte.
- **Iterators became functions** returning `String` and `Vec<u8>`. The origin's `Cow` results (borrowing the input when nothing changes), `Display` and `Iterator` impls are not ported.
- **The decoder's hex digits** are the origin's `char::to_digit(16)`, which is `hex::value`.

## What was left out

- `decode_utf8` and `decode_utf8_lossy` (UTF-8 validation of the decoded bytes, with `valid_up_to`/`error_len` and U+FFFD replacement): left out for scope; `std::utf8::valid_up_to` would be the base for them.
- The `Cow` conversions and the `alloc`/`std` features.

## Tests

The test suite is declared in `package.json` and run by `gotgan test`: gotgan builds `tests/driver.bmb` with `src/percent_encoding.bmb` and `hex`'s unit (`bmb build`, which proves everything first) and runs each case of the two case files (gotgan's format `ARGS|STDIN|STDOUT`).

- **`tests/cases.txt`, 14 cases**, from the origin's tests (`src/lib.rs` `mod tests` and `src/ascii_set.rs` `mod tests`) and its documentation examples.
  - The `Cow`, `Display` and iterator variants of a test have the same expected text and are listed once.
  - The UTF-8 tests check the decoded bytes.
  - The `AsciiSet` operator tests check the membership of every ASCII byte.
- **`tests/reference.txt`, 1,600 cases**, with the expected values computed by the origin crate itself (`tests/reference/`, a Cargo program depending on `percent-encoding = "=2.3.2"`):
  - for the nine sets of the driver, the membership of every ASCII byte and the encoding of all 256 bytes;
  - each byte encoded alone under `CONTROLS` and `NON_ALPHANUMERIC`;
  - the decoding of `%` before every byte and before every pair from a small alphabet;
  - 300 pseudo-random inputs encoded, with the round trip for the sets that hold `%`, and decoded text built mostly from well-formed and malformed `%`-sequences.

Result: 1,614 passed, 0 failed (`gotgan test`, 9 to 12 s on a loaded machine; it was 5 to 6 s before `hex`'s definitions were proved with it). Not exercised: inputs longer than what the driver reads in a test (the cases are short).

## The rewriting tool on this crate

Run as `rust2bmb src/lib.rs=percent_encoding src/ascii_set.rs`. The numbers are in [`tools/rust2bmb/README.md`](../../tools/rust2bmb/README.md#measured-on-percent-encoding-232).

- 451 code lines and 35 items: 18 written (1 with nothing reported), 17 dropped.
- 89 constructs reported and 87 rewrites applied.
- The raw output does not pass `bmb check`: 102 diagnostics.
- `src/percent_encoding.bmb` was written by hand. Of its 356 non-blank lines, 32 are identical to a line of the tool's output, and only the `mod` line is not a brace.
- The 356 lines:

  | kind | lines |
  | --- | --- |
  | `spec fn` | 83 |
  | contract clauses | 39 |
  | proof (assertions, `proof` blocks) | 22 |
  | `proof fn` headers | 7 |
  | `#[summary]` | 36 |
  | `#[uses]` | 17 |
  | `#[effects]` | 10 |
  | code | 142 |

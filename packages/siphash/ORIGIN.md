# Origin of the `siphash` package

## Source

- Crate: `siphasher` 1.0.4 (crates.io), "SipHash-2-4, SipHash-1-3 and 128-bit variants in pure Rust".
- Repository: <https://github.com/jedisct1/rust-siphash>, commit `ed028f01a16205b0819fa83b0cdc052e2d5ac3b7` (from the crate's `.cargo_vcs_info.json`).
- Files read: `src/lib.rs`, `src/sip.rs`, `src/sip128.rs`, `src/common.rs`, `src/tests.rs`, `src/tests128.rs`.
- Licence of the origin: `MIT OR Apache-2.0` (its `Cargo.toml`). This port uses it under the MIT licence. The origin's notice (`COPYING`) is

  ```
  Copyright 2012-2016 The Rust Project Developers.
  Copyright 2016-2026 Frank Denis.
  ```

  and its texts are kept unchanged in [`COPYING-origin`](COPYING-origin), [`LICENSE-MIT-origin`](LICENSE-MIT-origin) and [`LICENSE-APACHE-origin`](LICENSE-APACHE-origin). The port itself is under the repository's MIT licence.

## What was ported

| Origin (Rust) | Port (BMB, `src/siphash.bmb`) |
| --- | --- |
| `sip::SipHasher13`, `sip::SipHasher24`, `sip::SipHasher` (2-4) | `siphash::Hasher` with `rounds: Rounds::Sip13` or `Rounds::Sip24`, `wide: false` |
| `sip128::SipHasher13`, `sip128::SipHasher24` | the same `Hasher` with `wide: true` |
| `new_with_keys(k0, k1)`, `new_with_key(&[u8; 16])` | `new_with_keys(k0, k1, r)`, `new_with_key(key: [u8; 16], r)`, and `new128_with_keys`, `new128_with_key` |
| `hash(&self, bytes) -> u64` / `-> Hash128` | `hash(k0, k1, r, msg) -> u64`, `hash128(k0, k1, r, msg) -> Hash128` |
| `Hasher::write`, `Hasher::finish`, `Hasher128::finish128` | `write(&mut h, msg)`, `finish(&h)`, `finish128(&h)` |
| `key(&self) -> [u8; 16]`, `keys(&self) -> (u64, u64)` | `key(&h)`; the two words are the fields `h.k0`, `h.k1` (no tuples) |
| `Hash128 { h1, h2 }`, `as_bytes` | `siphash::Hash128 { h1, h2 }`; the bytes are `h1.to_le_bytes()` then `h2.to_le_bytes()` |

**The streaming interface is a plain struct with functions,** and the incremental state machine (the 0 to 7 pending bytes in `tail`, their count in `ntail`, the byte count `length`) is the origin's, field for field.

## Contracts

There is no short specification of SipHash other than the algorithm. The contracts therefore state the algorithm once, as `spec fn`s, and relate the code to it:

- `sipround`, `c_rounds`, `d_rounds`, `absorb` (one message word), `initial` (the keyed initial state) and `result`/`result128` (finalization) are the algorithm of the SipHash paper.
- `push(h, b)` is the state machine for one byte: it goes into the tail, and the eighth byte completes a word that is absorbed. `pushes(h, m, n)` is the first `n` bytes of `m`, one after another.
- **Proved:** `write_byte` is `push`; `write(h, msg)` leaves `pushes(old h, msg, msg.len())`; `finish` is `result`, `finish128` is `result128`; `hash(k0, k1, r, msg)` is `result(pushes(initial(k0, k1, r, false), msg, msg.len()))`, and the same for `hash128`; `round` is `sipround`; the constructors give `initial`. The state invariant `ok(h)` (fewer than 8 pending bytes, and the tail holds no more than they make up) is kept by every function.
- **Proved, streaming equals one-shot:** the lemma `streaming(h, x, a, b, m)`. If `m` is `a` followed by `b` and `x` is the hasher after writing `a`, then writing `b` to `x` leaves the hasher that writing `m` leaves. Combined with the contracts of `write` and `finish`, `finish(write(write(new(k), a), b)) == hash(k, a ++ b)`. Its two inductions are `prefix_same` and `suffix_same`.
- **Not stated:** the contract of `key` (`u64::from_le_bytes(r[0..8]) == h.k0` and the same for `k1`). The prover cannot decide `u64::from_le_bytes(x.to_le_bytes()) == x` (E0811, rlimit 5,000,000). The driver tests `key` instead.
- There is no precondition anywhere. `length` counts bytes modulo 2^64 (`wrapping_add`), which is what the origin's `usize` does in a release build and all that the output uses (its low byte).

**Proof cost** (Z3 4.15.4): `bmb verify src/siphash.bmb` proves 20 definitions with 109 obligations in 3.5 s, total rlimit 5,802,198.

- The largest are `write` (1,604,959), `suffix_same` (1,241,546, largest single query 780,348), `compress_d` (four SipRounds, 570,683), `streaming` (506,413), `prefix_same` (349,410), `write_byte` (321,908), `finish128` (315,969), `compress_c` (298,792) and `round` (186,899, 14 rotations, xors and wrapping additions). Everything else is below 150,000.
- `bmb stability` over seeds 0 to 4 finds no definition whose verdict depends on the seed (largest rlimit spread 3.11x, `round`: 60,015 to 186,899).
- The 64-bit rotations, xors and wrapping additions themselves are cheap. One SipRound proved equal to its specification costs at most 186,899. The expensive parts are the recursive state-machine specification and its inductions.

## What was changed, and why

- **`write` takes the message one byte at a time.** The origin fills the tail, then loads whole 64-bit words with `ptr::copy_nonoverlapping`, then keeps the rest. The port's word-at-a-time version needs the lemma "eight bytes written to an empty tail absorb their little-endian word" (`eight`).
  - With `wrapping_add` lengths it verified with seed 0 (rlimit about 46,000,000) but failed under seeds 1 and 4.
  - With plain additions it ran out of the solver's 1 GB.
  - With the intermediate hashers bound by nested spec `let`s, the compiler process itself was killed for lack of memory.
  - So the byte-at-a-time state machine was kept. It computes the same values, which the tests confirm against the origin. The word-at-a-time `write` is left for when the prover handles it.
- **Tail bytes are placed by addition, not `|`.** `tail + place_byte(b, n)`, where `place_byte` shifts by a constant per case, has the same value as the origin's `tail | (b as u64) << 8 * ntail`: the bits do not overlap. The arithmetic form is linear for the prover. The variable-shift form, checked with `assert(..) by (bit_vector)`, was rejected with a counterexample that treats `b as u64` as a variable of its own (E0818).
- **No tuples:** `keys()` returns `(u64, u64)` in the origin; here the fields `k0`, `k1` are read directly.
- **No `Hash`/`Hasher` traits:** `write_u8`/`write_u16`/`write_u32`/`write_u64`/`write_usize` and the `Hasher` impls are not ported. Writing the little-endian bytes is the same, since `short_write` only shortcuts that.
- **One generic `Hasher<S: Sip>` became one struct with an enum field.** The origin's `PhantomData<S>` and the trait `Sip` select the rounds at compile time; here `Rounds` selects them at run time, and `wide` the 128-bit variant (initial `v1 ^ 0xee`, two finalizations).

## What was left out

- serde support (the `serde*` features) and `reexports`.
- The `Hash`-trait tests of the origin (`test_hash_idempotent`, `test_hash_no_bytes_dropped_64`/`_32`, `test_hash_no_concat_alias`, `test_siphash128_idempotent`): they hash Rust values through `core::hash::Hash` (integers in native byte order, tuples, `str` with its 0xff terminator), which has no counterpart here.
- `Hash128`'s `From<u128>`, `as_u128` and `as_u64` (BMB has no `u128`, and `as_u64` returns a tuple).

## Tests

`tests/run.sh` builds `tests/driver.bmb` with `src/siphash.bmb` (`bmb build`, which proves everything first). It sends each case's key and message, hex in the data, to the executable's standard input, and compares its output with the expected little-endian hex.

- `tests/cases.txt`, 260 cases, from the origin's `src/tests.rs` and `src/tests128.rs`:
  - the 64 SipHash-2-4 and 64 SipHash-1-3 vectors of the reference implementation (message `00 01 .. t-1`, key `00 .. 0f`);
  - the SipHash128-2-4 and SipHash128-1-3 vectors;
  - each checked one-shot and written one byte at a time, as the origin's tests do. The data was transcribed from the origin's source by a script.
- `tests/reference.txt`, 603 cases, with the expected values computed by the origin crate itself (`tests/reference/`, a Cargo program depending on `siphasher = "=1.0.4"`):
  - the origin's `test_hash_simple` and `test_hash_incremental` inputs, which the origin runs without asserting a value;
  - `key`;
  - 400 one-shot hashes of pseudo-random keys and messages (0 to 1023 bytes, all four variants);
  - 200 incremental hashes with pseudo-random piece lengths, where the generator also checks that the origin's own streaming and one-shot results agree.

Result: 863 passed, 0 failed (about 12 s). A wrong expected value is reported (checked by editing one).

Not exercised: messages longer than 2^20 bytes in pieces (the driver's limit), and lengths of 2^64 bytes or more. The proof covers every input.

## The rewriting tool on this crate

Run as `rust2bmb src/lib.rs=siphasher src/sip.rs src/sip128.rs src/common.rs` (the test files are data, not ported code). The numbers and the counted list of reported constructs are in [`tools/rust2bmb/README.md`](../../tools/rust2bmb/README.md#measured-on-siphasher-104).

- 987 code lines and 74 items: 44 written (5 with nothing reported), 30 dropped.
- 148 constructs reported; 564 rewrites applied, 49 of them `let` types the tool's new inference wrote.
- The raw output does not pass `bmb check`: 347 diagnostics, where the unit `siphasher` (only `mod` items) checks.
- `src/siphash.bmb` was written by hand. Of its 363 non-blank lines, 52 are identical to a line of the tool's output, all of them braces or `} else {`: the origin's SipRound is a macro the tool drops, its word loads are `unsafe` macros, and the port's byte-at-a-time state machine has no counterpart line in the origin.
- The 363 lines: 105 lines of `spec fn`, 30 contract clauses, 32 `#[summary]`, 16 `#[effects]`, 12 `#[uses]`, and 165 lines of code and proof functions.

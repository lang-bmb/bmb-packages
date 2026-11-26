# Origin of the `hmac` package

## Source

- Crate: `hmac` 0.13.0 (crates.io), RustCrypto's "Generic implementation of Hash-based Message Authentication Code (HMAC)".
- Repository: <https://github.com/RustCrypto/MACs> (directory `hmac`), commit `0236c8eb50098dd7f277a71ab89caaeb1e7314df` (from the crate's `.cargo_vcs_info.json`).
- Files read: `src/lib.rs`, `src/simple.rs`, `src/simple_reset.rs`, `src/block_api.rs`, `src/utils.rs`, `tests/mod.rs`, `tests/data/hmac_sha256_rfc4231.blb`.
- Licence of the origin: `MIT OR Apache-2.0` (its `Cargo.toml`). This port uses it under the MIT licence. The origin's MIT text carries the notice `Copyright (c) 2017 Artyom Pavlov`. Its two licence texts are kept unchanged in [`LICENSE-MIT-origin`](LICENSE-MIT-origin) and [`LICENSE-APACHE-origin`](LICENSE-APACHE-origin). The port itself is under the repository's MIT licence.
- The origin is generic over any hash function of the `digest` crate. The port is HMAC-SHA256 only, on top of the standard library's `std::sha256`. The interface shape (key-derivation `get_der_key`, `IPAD`/`OPAD`, `verify_slice`, `verify_truncated_left`) and the test vectors come from the origin.

## What was ported

| Origin (Rust) | Port (BMB, `src/hmac.bmb`) | Contract |
| --- | --- | --- |
| `get_der_key` (key padded with zeros, or hashed and padded when longer than the block) | `key_hash(key)`, then `key_block(key, kh)` | exact: `key_block_of(key, &kh, &kb)`, with `std::sha256::hashed(key, &kh)` for a key longer than 64 bytes |
| the `IPAD`/`OPAD` blocks and the inner and outer updates | `pad_input(kb, pad, data)` | exact: the 64 bytes `kb ^ pad`, then `data` |
| `Hmac::<Sha256>::new_from_slice(key)`, `update(msg)`, `finalize()` | `mac(key, msg) -> [u8; 32]` | the body proves the tag is HMAC-SHA256 (below); the contract states the input lengths only |
| `Mac::verify_slice(tag)` | `verify_slice(expected, tag)` | exact: a 32-byte tag equal to `expected` |
| `Mac::verify_truncated_left(tag)` | `verify_truncated_left(expected, tag)` | exact: 1 to 32 bytes equal to the left of `expected` |

## Contracts and what `std::sha256` offers

`std::sha256` offers one function and its specification:

- `std::sha256::digest(msg: &[u8]) -> (r: [u8; 32])`, with `requires msg_ok(msg)` (fewer than 2^61 bytes) and `ensures hashed(msg, &r)`.
- The specification `hashed(msg, r)`: `r` is the FIPS 180-4 digest of `msg`, defined through `chain`, `padlen`, `bytes_of` and the round functions. All of them are `spec fn`s a package can name.

There is no incremental interface (`update`/`finalize`). So the inner input `(K' ^ ipad) ++ msg` is built in memory, a copy of the message, where the origin streams it. The definition of HMAC is stated in terms of `hashed`:

```
is_hmac(key, msg, kh, kb, inner, ih, outer, tag) =
    key_block_of(key, kh, kb)                 // K'
    && padded_with(kb, 0x36, msg, inner)      // inner = (K' ^ ipad) ++ msg
    && std::sha256::hashed(inner, ih)         // ih = H(inner)
    && padded_with(kb, 0x5c, ih, outer)       // outer = (K' ^ opad) ++ ih
    && std::sha256::hashed(outer, tag)        // tag = H(outer)
```

`mac` ends with `assert(is_hmac(key, msg, &kh, &kb, inner.as_slice(), &ih, outer.as_slice(), &tag))`, and the gate proves it. So every tag `mac` returns is HMAC-SHA256(key, msg) as RFC 2104 defines it, in terms of the SHA-256 specification of the standard library.

**Wanted and not stated:** `mac`'s `ensures` cannot say the same, because the intermediate values are its locals. Three ways to export them were tried, and each was refused or failed:

1. Return a struct holding them: a field array cannot be passed to a `spec fn` as a slice (`&r.tag`, `r.inner.as_slice()`: E0409, "expected `&[u8]` of a variable", in `ensures` and in a `spec fn` body alike).
2. Write them to `&mut [u8]` out-parameters (`kh`, `ih`): `hashed` does not carry from the digest array to an equal copy. With `#[uses(std::sha256::hashed, std::sha256::bytes_of)]`, `copy` of a 32-byte digest is E0802, with a counterexample in which the two arrays are equal. A relation of the same shape defined locally verifies. With more of `std::sha256` revealed it is E0811.
3. Arrays by reference (`&mut [u8; 32]`): not a BMB parameter type (E0402).

So the exact statement lives in `mac`'s body, and each step (`key_hash`, `key_block`, `pad_input`, `std::sha256::digest`) carries an exact contract a caller can compose the same way.

**What was missing from `std::sha256`:**

- an incremental interface;
- a statement of the digest of a concatenation (here the concatenation is materialized);
- a way for `hashed` to survive a copy of the digest (or a lemma `hashed(m, a) && same(a, b) ==> hashed(m, b)`).

**Proof cost** (Z3 4.15.4): `bmb verify src/hmac.bmb` proves the 8 definitions of the unit (72 obligations, total rlimit 487,146; 30 definitions with `std::sha256`) in 1.8 s.

- The largest are `pad_input` (195,950) and `key_block` (192,489).
- `bmb stability` over seeds 0 to 4: no definition depends on the seed (largest spread 1.87x).

## What was changed, and why

- **SHA-256 only**, over `std::sha256`: BMB has no `digest` crate, no associated types (`BlockSizeUser::BlockSize`, `OutputSizeUser::OutputSize`) and no generic array types. A package cannot depend on another package yet.
- **One-shot instead of `update`/`finalize`**: `std::sha256` is one-shot. The origin's `Hmac`, `HmacReset`, `SimpleHmac` and `SimpleHmacReset` (four structs, all computing the same tag) become the one function `mac`; reset has nothing to keep.
- **The tag comparison is a loop that reads every byte**, `diff |= a ^ b`, as the origin's `subtle::ConstantTimeEq` does. That the generated code runs in time independent of the data is not verified.
- **The xor with the pad is a named spec function `xr`** in the quantified facts. With `kb[q] ^ pad` written directly in a loop invariant's `forall`, the invariant could not be proved (E0811, rlimit 5,000,000), although it is the same fact.

## What was left out

- Other hash functions (MD5, SHA-1, SHA-224/384/512, Streebog) and the Wycheproof and GOST test files that exercise them.
- `KeyInit`, `Mac`, `Update`, `FixedOutput`, `Reset`, `AlgorithmName`, `Debug` impls (traits of the `digest` crate).
- The block-level API (`block_api.rs`: `HmacCore` over `EagerHash`).

## Tests

The test suite is declared in `package.json` and run by `gotgan test`. gotgan builds `tests/driver.bmb` with `src/hmac.bmb` (`bmb build`, which proves everything first). Each case (gotgan's format `ARGS|STDIN|STDOUT`) gives the operation and the key and message lengths as the arguments, and the key, the message and the tag, written byte for byte as `\xHH`, as the standard input; the output must be the expected tag hex, or `ok`/`error`.

- `tests/cases.txt`, 16 cases: RFC 4231 §4 test cases 1 to 7 (HMAC-SHA-256), decoded from the origin's `tests/data/hmac_sha256_rfc4231.blb` with the `blobby` crate.
  - Each tag is computed (`mac`) and verified (`verify_slice`).
  - Test case 5, which the RFC specifies truncated to 128 bits, is checked with `verify_truncated_left` on its first 16 bytes, and rejected by `verify_slice`.
- `tests/reference.txt`, 436 cases, with the expected values computed by the origin crate over `sha2` (`tests/reference/`, a Cargo program depending on `hmac = "=0.13.0"`):
  - 300 tags for pseudo-random keys (including exactly 64 and 65 bytes, up to 199) and messages (up to 1999 bytes);
  - `verify_slice` and `verify_truncated_left` for every tag length from 0 to 33, with the right bytes and with the last byte changed.

Result: 452 passed, 0 failed (`gotgan test`, about 3 s). A wrong expected value fails it (checked by editing one).

Not exercised: keys or messages longer than the driver's 2^20 bytes, and the constant-time property.

## The rewriting tool on this crate

Run as `rust2bmb src/lib.rs=hmac src/simple.rs src/simple_reset.rs src/block_api.rs src/utils.rs`. The numbers and the counted list of reported constructs are in [`tools/rust2bmb/README.md`](../../tools/rust2bmb/README.md#measured-on-hmac-0130).

- 359 code lines and 70 items: 42 written (2 with nothing reported), 28 dropped.
- 285 constructs reported, 157 of them uses of `digest` items; 89 rewrites.
- The raw output does not pass `bmb check`: 98 diagnostics.
- `src/hmac.bmb` was written by hand. Of its 161 non-blank lines, 30 are identical to a line of the tool's output: 29 of them are braces or `} else {`, and the other is the `mod hmac {` line.
- The 161 lines: 27 lines of `spec fn`, 20 contract clauses, 2 lines of proof, 15 `#[summary]`, 8 `#[effects]`, 7 `#[uses]`, 82 lines of code.

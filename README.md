# bmb-packages

The official packages of the BMB language.

BMB is a language with a Rust-shaped surface whose functions carry contracts that an SMT solver proves before any code is built. The packages here are ports of widely used Rust crates. Each one keeps the origin's tests and published test vectors as data, and its public functions carry functional contracts where an exact one is short.

**Status.** For now these packages are working material for the language's own development and measurement: they are written to find where the language, the compiler and the tools fall short, and to measure the cost of proof on real code. They are not yet declared usable by others, and their interfaces will change. There is not yet a way for a package to call another package; a package may call the standard library (`std::NAME::f`).

## Packages

| Package | Origin | What it is |
| --- | --- | --- |
| [`hex`](packages/hex) | the Rust crate `hex` 0.4.3 | bytes as hexadecimal text and back (RFC 4648 base16), with a proved round trip |
| [`siphash`](packages/siphash) | the Rust crate `siphasher` 1.0.4 | SipHash-2-4 and SipHash-1-3, 64- and 128-bit output, one-shot and incremental, with writing in pieces proved equal to writing the whole |
| [`hmac`](packages/hmac) | the Rust crate `hmac` 0.13.0 | HMAC-SHA256 on top of `std::sha256`, proved to be HMAC in terms of the SHA-256 specification |

## Layout of a package

```
packages/<name>/
  package.json    name, version, summary, units, the std units it uses, origin (crate, version, repository, licence),
                  and the test suites (tests: the test units and the case files)
  src/<unit>.bmb  one file per unit, in normal form (bmb fmt)
  tests/          the test program (tests/driver.bmb) and the origin's tests and vectors as data, in gotgan's
                  case format ARGS|STDIN|STDOUT (cases.txt, reference.txt); tests/reference/ (when present) is
                  the Cargo program that computed tests/reference.txt with the origin crate
  ORIGIN.md       where it comes from, its licence and attribution, what was changed in porting and why, what was left out
```

The packages are checked, proved and tested with [gotgan](https://github.com/lang-bmb/gotgan), the BMB package manager, which reads `package.json`. Each package is accepted when:

1. `gotgan verify` passes: `bmb verify` proves every definition, with no escape hatch (no `assume`, no `#[external_body]`), no trusted definition and no unproved termination, and the standard library units the program reaches are exactly `deps.std`;
2. `gotgan check` passes: its sources and its test units are in normal form (`bmb fmt --check`) and check;
3. `gotgan test` passes: for each test suite of `package.json`, it builds the driver with `bmb build` (which proves it first) and runs every case of the case files through it; a case passes when the driver exits with 0 and writes exactly the expected bytes.

## Checking

```
BMB=/path/to/bmb GOTGAN=/path/to/gotgan scripts/check.sh          # every package
BMB=/path/to/bmb GOTGAN=/path/to/gotgan scripts/check.sh hex      # one package
```

`scripts/check.sh` runs `gotgan check`, `gotgan verify` and `gotgan test` on each package, one after another, prints one summary line per command (definitions and obligations proved, cases passed and failed, taken from gotgan's JSON rows) and every failing case in full, and exits non-zero if any command fails. `BMB` and `GOTGAN` default to `bmb` and `gotgan` on `PATH`. gotgan is built from its repository with `bmb build src/*.bmb -o gotgan` (see its README). Building executables needs `clang` and `llc`, and proving needs `z3`. By hand, for one package:

```
gotgan --package packages/hex check
gotgan --package packages/hex verify
gotgan --package packages/hex test
```

Results are JSON rows on standard output; gotgan writes the test drivers to `packages/<name>/target/gotgan/`.

## Tools

[`tools/rust2bmb`](tools/rust2bmb) rewrites a Rust source file into the shape of a BMB unit and reports, with file and line, every construct it does not rewrite. It writes no contracts. It is the first step of a port, and its reports measure which Rust idioms fall outside the BMB surface; the numbers for each port are in the tool's README and in the package's `ORIGIN.md`.

## Licence

MIT (see [LICENSE](LICENSE)). A package ported from another project also keeps that project's licence texts and notices in its directory; see its `ORIGIN.md`.

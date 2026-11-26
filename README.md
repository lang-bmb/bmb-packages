# bmb-packages

The official packages of the BMB language.

BMB is a language with a Rust-shaped surface whose functions carry contracts that an SMT solver proves before any code is built. The packages here are ports of widely used Rust crates. Each one keeps the origin's tests and published test vectors as data, and its public functions carry functional contracts where an exact one is short.

**Status.** For now these packages are working material for the language's own development and measurement: they are written to find where the language, the compiler and the tools fall short, and to measure the cost of proof on real code. They are not yet declared usable by others, and their interfaces will change. There is not yet a way for a package to call another package; a package may call the standard library (`std::NAME::f`).

## Packages

| Package | Origin | What it is |
| --- | --- | --- |
| [`hex`](packages/hex) | the Rust crate `hex` 0.4.3 | bytes as hexadecimal text and back (RFC 4648 base16), with a proved round trip |

## Layout of a package

```
packages/<name>/
  package.json    name, version, summary, units, the std units it uses, origin (crate, version, repository, licence)
  src/<unit>.bmb  one file per unit, in normal form (bmb fmt)
  tests/          the origin's tests and vectors as data, the program that runs them, and tests/run.sh
  ORIGIN.md       where it comes from, its licence and attribution, what was changed in porting and why, what was left out
```

Each package is accepted when:

1. `bmb verify` proves every definition with no escape hatch (no `assume`, no `#[external_body]`, no unproved termination);
2. its sources and test programs are in normal form (`bmb fmt --check`);
3. its tests pass: `tests/run.sh` builds the test program with `bmb build` (which proves it first) and runs it on the data, exiting non-zero on any mismatch.

## Checking

```
BMB=/path/to/bmb scripts/check.sh          # every package
BMB=/path/to/bmb scripts/check.sh hex      # one package
```

`BMB` defaults to `bmb` on `PATH`. Building executables needs `clang` and `llc`, and proving needs `z3`. By hand, for one package:

```
bmb fmt --check packages/hex/src/hex.bmb
bmb verify packages/hex/src/hex.bmb
BMB=bmb packages/hex/tests/run.sh
```

## Tools

[`tools/rust2bmb`](tools/rust2bmb) rewrites a Rust source file into the shape of a BMB unit and reports, with file and line, every construct it does not rewrite. It writes no contracts. It is the first step of a port, and its reports measure which Rust idioms fall outside the BMB surface; the numbers for each port are in the tool's README and in the package's `ORIGIN.md`.

## Licence

MIT (see [LICENSE](LICENSE)). A package ported from another project also keeps that project's licence texts and notices in its directory; see its `ORIGIN.md`.

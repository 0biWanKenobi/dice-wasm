# Chumsky Dice WASM (starter)

A Rust/Chumsky parser and evaluator compiled to a TypeScript-importable WASM package using Docker Compose.

## Build

```bash
docker compose run --rm wasm-build
```

Build output appears under `./dist/` (the `bundler` target supported by Vite, webpack, etc.). The first build downloads Rust crates and wasm-pack via the container.

## TypeScript / Vite

In the TS application:

```bash
npm install /absolute/path/to/chumsky-dice-wasm/dist
```

```ts
import { CompiledFormula } from 'chumsky-dice';

const formula = new CompiledFormula('kh(4d8, 2) + PROF');
console.log(formula.dependencies()); // ['PROF']
console.log(formula.ast());
console.log(formula.evaluate({ PROF: 2 })); // { value: number, dice: number[][] }
formula.free(); // When no longer needed
```

`CompiledFormula` retains its AST between evaluation calls; your TypeScript reactive dependency graph should continue to decide when to reevaluate.

## Syntax

- Numbers, uppercase variable names, parentheses, `+ - * /`, comparison operators
- Compact dice notation `3d6`, `DEXd6` and `(STR + 2)d(DEX + 4)`
- Functions: `floor`, `ceil`, `abs`, `min`, `max`, `kh`, `dl`
- `kh` and `dl` currently expect a bare dice expression as their first argument, e.g. `kh(4d8,2)`. The full roll group is preserved in `dice`, but kept/dropped flags are not yet separately returned.

## Known limitations

- **Not locally compiled or validated**: this environment has no Rust/Cargo/Docker or network access. The crate is provided as a starting implementation; compilation fixes may be necessary, particularly with Chumsky 0.13's evolving APIs.
- To distinguish `DEXd6` unambiguously from a field named `DEXd6`, variables are currently uppercase (`STR_MOD`, `CHARACTER.STR`) and function names lowercase (`floor`, `min`). Lowercase variable identifiers are not supported yet.
- User-defined function names require adding handlers in Rust; no dynamic JS callback registration yet.
- A formula containing dice reevaluates dice every call; invoke `evaluate()` only when a roll is requested for such formulas.
- Randomness currently uses `Math.random()`, not cryptographic randomness.
- Numeric inputs are `f64` and booleans can be evaluation results; boolean variables are not supported.

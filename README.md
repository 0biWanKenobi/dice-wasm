# dice-wasm

A browser-compatible Rust/WASM formula and dice engine using [Chumsky](https://github.com/zesterer/chumsky).
The npm package is published as **`@0biwankenobi/dice-wasm`**.

## Build locally

```sh
docker compose run --rm wasm-build
node scripts/prepare-package.mjs
node scripts/check-package.mjs
```

The output is in `dist/` (ES modules, TypeScript declarations, and the WASM binary).
The `bundler` target is designed for Vite, webpack, and similar bundlers.

## Consumer usage

After npm publication:

```sh
npm install @0biwankenobi/dice-wasm
```

```ts
import { CompiledFormula } from '@0biwankenobi/dice-wasm';

const formula = new CompiledFormula('kh(4d8, 2) + 3');
console.log(formula.dependencies());
console.log(formula.rollable()); // true
const result = formula.roll();
console.log(result); // { value, dice: [{ rolls, kept, total }] }
formula.free();
```

The parser supports braced references such as `{STR}`, expressions as either
dice operand (`({STR}+1)d(max(6,8))`), mathematical functions, comparisons,
conditionals, and `kh` / `dl`. The consuming application must resolve nested
references before evaluating; this engine does not manage a reactive graph.

## Release workflow

`.github/workflows/release.yml` runs Rust tests, builds the WASM package,
checks its generated public API, and uploads a build artifact on pushes.
On a version tag (e.g. `v0.1.0`), it additionally publishes to npm and creates
a GitHub release with a package tarball. The tag must match the version in
`Cargo.toml`.

**First-time npm setup (one-time):**

1. Ensure you control the `@0biwankenobi` namespace on npm. The npm account
   need not be the same as the GitHub account, but it must own this scope.
2. npm requires the package to exist before a trusted publisher can be configured.
   Publish the **first** package manually with `npm publish --access public` from
   the generated `dist/` after a successful build, while authenticated on npm.
3. On the package's npm settings, configure a GitHub Actions trusted publisher:
   GitHub owner `0biWanKenobi`, repository `dice-wasm`, workflow filename
   `release.yml`, with direct `npm publish` permission.
4. For the first `v0.1.0` release, if version 0.1.0 was used during bootstrap,
   **do not publish the same version again**: bump `Cargo.toml` to 0.1.1 and
   tag `v0.1.1`. npm versions are immutable.
5. Future releases: update crate version, commit it, and push matching
   `vX.Y.Z` tags. No long-lived npm token is needed.

The consumer repository is
[ts_reactive_character_sheet_test](https://github.com/0biWanKenobi/ts_reactive_character_sheet_test/tree/rust-wasm-engine).

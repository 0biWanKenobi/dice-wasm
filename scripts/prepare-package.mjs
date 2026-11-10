import { readFileSync, writeFileSync } from 'node:fs';

const path = new URL('../dist/package.json', import.meta.url);
const pkg = JSON.parse(readFileSync(path, 'utf8'));
pkg.name = '@0biwankenobi/dice-wasm';
pkg.description = 'Chumsky-powered dice expression parser and WASM evaluator';
pkg.license = 'MIT';
pkg.repository = {
  type: 'git',
  url: 'git+https://github.com/0biWanKenobi/dice-wasm.git'
};
pkg.homepage = 'https://github.com/0biWanKenobi/dice-wasm';
// The package is distributed through GitHub Releases, not a registry.
delete pkg.publishConfig;
// Keep wasm-pack's generated entry points, declarations and files list.
writeFileSync(path, JSON.stringify(pkg, null, 2) + '\n');
console.log(`Prepared ${pkg.name}@${pkg.version}`);

import { readFileSync, existsSync } from 'node:fs';

const pkg = JSON.parse(readFileSync('dist/package.json', 'utf8'));
if (pkg.name !== '@0biwankenobi/dice-wasm') throw new Error('Unexpected npm package name');
for (const file of [
  'chumsky_dice.js',
  'chumsky_dice.d.ts',
  'chumsky_dice_bg.wasm'
]) {
  if (!existsSync('dist/' + file)) throw new Error('Missing generated file: ' + file);
}
const declarations = readFileSync('dist/chumsky_dice.d.ts', 'utf8');
for (const method of ['CompiledFormula', 'FormulaContext', 'dependencies', 'rollable', 'evaluate', 'resolve', 'set', 'remove', 'roll']) {
  if (!declarations.includes(method)) throw new Error('Missing WASM API: ' + method);
}
console.log('Generated npm package API looks correct');

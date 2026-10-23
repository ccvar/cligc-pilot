// Source helpers for tests that exercise real app handlers in a vm sandbox.
// The app shell is split into the route page, its components (src/lib/components)
// and rune stores with their logic (src/lib/state); tests read those files directly.
import fs from 'node:fs';
import ts from 'typescript';

const SRC = new URL('../../', import.meta.url);

function list(dir, ext) {
  const root = new URL(dir, SRC);
  if (!fs.existsSync(root)) return [];
  return fs.readdirSync(root, { recursive: true }).filter((f) => f.endsWith(ext)).sort().map((f) => `${dir}${f}`);
}

/** Read a file relative to src/. */
export function read(rel) {
  return fs.readFileSync(new URL(rel, SRC), 'utf8');
}

/** State modules (rune stores and the logic that owns them). */
export const STATE_FILES = list('lib/state/', '.ts');

/** Every file that makes up the app shell, for assertions about the app as a whole. */
export function appSource() {
  return ['routes/+page.svelte', ...list('lib/components/', '.svelte'), ...STATE_FILES].map(read).join('\n');
}

/** Strip `export` so module code can run as a vm script. */
export function asScript(text) {
  return text.replace(/^export (?=(?:async )?function |const |let |class |type )/gm, '');
}

/** Text of `rel` from the first `from` marker up to the following `to` marker, runnable as a script. */
export function sliceSource(rel, from, to) {
  const source = read(rel);
  const a = source.indexOf(from), b = source.indexOf(to, a);
  if (a < 0 || b <= a) throw new Error(`markers not found in ${rel}: ${from} … ${to}`);
  return asScript(source.slice(a, b));
}

let declarations;
/** Top-level function declarations from the state modules, by name, runnable as a script. */
export function stateFunctions(names) {
  if (!declarations) {
    declarations = new Map();
    for (const rel of STATE_FILES) {
      const sf = ts.createSourceFile(rel, read(rel), ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
      for (const node of sf.statements) {
        if (ts.isFunctionDeclaration(node) && node.name) declarations.set(node.name.text, asScript(node.getText(sf)));
      }
    }
  }
  return names.map((name) => {
    if (!declarations.has(name)) throw new Error(`Missing app function: ${name}`);
    return declarations.get(name);
  }).join('\n');
}

/** Names of the exported rune stores (`export const app = new AppState()` …). */
export const STORES = STATE_FILES.flatMap((rel) => [...read(rel).matchAll(/^export const (\w+) = new \w+\(\);$/gm)].map((m) => m[1]));

/**
 * Let a flat vm context stand in for every store: code reads `app.conns`,
 * `launcher.lPerm` … while the test keeps setting and asserting `ctx.conns`, `ctx.lPerm`.
 */
export function withStores(ctx) {
  for (const store of STORES) ctx[store] = ctx;
  return ctx;
}

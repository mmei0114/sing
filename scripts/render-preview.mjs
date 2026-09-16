// Generate a documentation asset from the real, fictional-data preview.
// No API calls, user configuration, or proxy connection are involved.
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const lines = execFileSync(path.join(root, 'sing'), ['--preview'], {
  cwd: root, encoding: 'utf8', maxBuffer: 1024 * 1024,
}).trimEnd().split('\n').map(line => line.trimEnd());
if (!lines.some(line => line.includes('DEMO')) || lines.length !== 30) {
  throw new Error('Expected the 110×30 fictional preview; inspect changes before regenerating.');
}
const escape = s => s.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
const rows = lines.map((line, i) => {
  const color = i === 2 ? '#8ce4d1' : (i === 1 || i === 3 || i >= 28) ? '#97baff' : '#dbe3f0';
  return `<text x="24" y="${74 + i * 20}" fill="${color}" xml:space="preserve">${escape(line)}</text>`;
}).join('\n');
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1120" height="682" viewBox="0 0 1120 682" role="img" aria-labelledby="title desc">
<title id="title">sing 0.6.0 — real terminal preview</title>
<desc id="desc">The Overview screen rendered by sing --preview. Fictional stopped demo with no live traffic. Five workspaces, import and setup actions, core and capture status, and Review Changes.</desc>
<rect width="1120" height="682" rx="14" fill="#0f1724"/>
<path d="M14 0h1092a14 14 0 0 1 14 14v28H0V14A14 14 0 0 1 14 0" fill="#1b2738"/>
<circle cx="25" cy="21" r="5" fill="#fa7d86"/><circle cx="43" cy="21" r="5" fill="#f2cc72"/><circle cx="61" cy="21" r="5" fill="#78dba9"/>
<text x="560" y="26" text-anchor="middle" fill="#b4c4dc" font-size="13" font-family="monospace">sing · fictional demo · 110 × 30</text>
<g font-family="Menlo,Consolas,DejaVu Sans Mono,monospace" font-size="16">${rows}</g>
</svg>\n`;
const directory = path.join(root, 'docs/assets');
mkdirSync(directory, { recursive: true });
writeFileSync(path.join(directory, 'overview.svg'), svg);
console.log('Rendered docs/assets/overview.svg from sing --preview.');

// Dependency-free local Markdown link check. External URLs are checked separately.
import { readFileSync, existsSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const markdown = [];
for (const dir of ['', 'docs', '.github']) {
  function walk(base) {
    for (const entry of readdirSync(base, { withFileTypes: true })) {
      const file = path.join(base, entry.name);
      if (entry.isDirectory()) { if (dir) walk(file); }
      else if (entry.name.endsWith('.md')) markdown.push(file);
    }
  }
  walk(path.join(root, dir));
}
let count = 0;
const errors = [];
for (const file of markdown) {
  const text = readFileSync(file, 'utf8').replace(/```[^]*?```/g, '');
  for (const match of text.matchAll(/\]\(([^\s)]+)(?:\s+"[^"]*")?\)/g)) {
    const url = match[1];
    if (/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(url)) continue;
    count++;
    const [relative, fragment] = url.split('#');
    const target = relative ? path.resolve(path.dirname(file), decodeURIComponent(relative)) : file;
    if (!target.startsWith(root) || !existsSync(target)) {
      errors.push(`${path.relative(root, file)}: missing local target ${url}`); continue;
    }
    if (fragment && target.endsWith('.md') && statSync(target).isFile()) {
      const headings = [...readFileSync(target, 'utf8').matchAll(/^#{1,6}\s+(.+)$/gm)]
        .map(m => m[1].toLowerCase().replace(/[^\p{L}\p{N}_\-\s]/gu, '').replace(/\s/g, '-'));
      if (!headings.includes(decodeURIComponent(fragment))) errors.push(`${path.relative(root, file)}: missing heading ${url}`);
    }
  }
}
if (errors.length) { console.error(errors.join('\n')); process.exit(1); }
console.log(`Checked ${count} local links in ${markdown.length} Markdown files.`);

import { readdirSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));
const files = readdirSync(path.join(root, 'src'), { recursive: true })
  .filter(file => file.endsWith('.test.ts')).sort();
if (!files.length) throw new Error('No frontend test files found');
for (const file of files) {
  const relative = path.join('src', file);
  const runner = readFileSync(path.join(root, relative), 'utf8').includes('node:test') ? 'test' : 'run';
  console.log(`Test file: ${relative}`);
  // Each file gets its own process: browser mocks and imported stores must not leak between tests.
  const result = spawnSync('bun', [runner, relative], { cwd: root, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
console.log(`All ${files.length} frontend test files passed`);

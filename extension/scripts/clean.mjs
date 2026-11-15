import { rmSync } from 'node:fs';
import { resolve } from 'node:path';

const target = resolve(process.cwd(), 'build');
rmSync(target, { recursive: true, force: true });
process.stdout.write('cleaned ' + target + '\n');

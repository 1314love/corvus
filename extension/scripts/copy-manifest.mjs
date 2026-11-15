import { copyFile, cp, mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';

const source = resolve(process.cwd(), 'manifest.json');
const outputDir = resolve(process.cwd(), 'build/extension');
const output = resolve(outputDir, 'manifest.json');

await mkdir(outputDir, { recursive: true });
await copyFile(source, output);
await cp(resolve(process.cwd(), '_locales'), resolve(outputDir, '_locales'), { recursive: true });
console.log('manifest and _locales copied to', outputDir);

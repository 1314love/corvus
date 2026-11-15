import { cp, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const DEFAULT_DAEMON = 'ws://127.0.0.1:7330';
const DEFAULT_PORTS = '[9222, 9223, 9333]';

async function collectTextFiles(root: string): Promise<string[]> {
  const found: string[] = [];
  const entries = await readdir(root, { withFileTypes: true });
  for (const entry of entries) {
    const full = join(root, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await collectTextFiles(full)));
      continue;
    }
    if (/\.(js|html|json|css)$/.test(entry.name)) found.push(full);
  }
  return found;
}

export interface PreparedExtension {
  dir: string;
  daemonReplacements: number;
  portReplacements: number;
  cleanup: () => Promise<void>;
}

export async function prepareIsolatedExtension(
  buildDir: string,
  daemonPort: number,
  cdpProbePort: number,
): Promise<PreparedExtension> {
  const root = await mkdtemp(join(tmpdir(), 'corvus-e2e-extension-'));
  const dir = join(root, 'extension');
  await cp(buildDir, dir, { recursive: true });

  let daemonReplacements = 0;
  let portReplacements = 0;
  for (const file of await collectTextFiles(dir)) {
    const before = await readFile(file, 'utf8');
    let after = before;
    if (before.indexOf(DEFAULT_DAEMON) !== -1) {
      daemonReplacements += before.split(DEFAULT_DAEMON).length - 1;
      after = after.split(DEFAULT_DAEMON).join('ws://127.0.0.1:' + String(daemonPort));
    }

    if (after.indexOf(DEFAULT_PORTS) !== -1) {
      portReplacements += after.split(DEFAULT_PORTS).length - 1;
      const ports = '[' + [cdpProbePort, cdpProbePort, cdpProbePort].join(', ') + ']';
      after = after.split(DEFAULT_PORTS).join(ports);
    }

    if (after !== before) await writeFile(file, after, 'utf8');
  }

  if (daemonReplacements === 0) throw new Error('daemon URL literal was not found in the build');
  if (portReplacements === 0) throw new Error('CDP candidate port literal was not found in the build');

  return {
    dir,
    daemonReplacements,
    portReplacements,
    cleanup: () => rm(root, { recursive: true, force: true }),
  };
}

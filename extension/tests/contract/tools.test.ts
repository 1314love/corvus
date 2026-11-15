import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { TOOL_NAMES, ToolArgsSchemas, parseToolArgs } from '../../src/shared/contracts/tools.generated';

interface SnapshotTool {
  name: string;
  inputSchema: Record<string, unknown>;
}

const snapshotPath = resolve(process.cwd(), '../tests/conformance/schema-snapshot.v1.5.0.json');
const snapshot = JSON.parse(readFileSync(snapshotPath, 'utf8')) as SnapshotTool[];

describe('generated tool contracts', () => {
  it('covers the schema snapshot exactly, in order', () => {
    expect(snapshot).toHaveLength(43);
    expect(Array.from(TOOL_NAMES)).toEqual(snapshot.map((tool) => tool.name));
    expect(Object.keys(ToolArgsSchemas)).toHaveLength(43);
    for (const tool of snapshot) {
      expect(ToolArgsSchemas[tool.name as keyof typeof ToolArgsSchemas], tool.name).toBeDefined();
    }
  });

  it('parses a valid command and applies strict unknown-key rejection', () => {
    expect(parseToolArgs('browser_navigate', { url: 'https://example.test' })).toEqual({ url: 'https://example.test' });
    expect(() => parseToolArgs('browser_navigate', {})).toThrow();
    expect(() => parseToolArgs('browser_navigate', { url: 'https://example.test', extra: true })).toThrow();
    expect(() => parseToolArgs('browser_click', { target: '@e1' })).not.toThrow();
  });

  it('enforces page_snapshot bounds and enum values', () => {
    expect(() =>
      parseToolArgs('page_snapshot', { format: 'compact', max_nodes: 300, viewport_only: true, include_bbox: true }),
    ).not.toThrow();
    expect(() => parseToolArgs('page_snapshot', { format: 'json' })).not.toThrow();
    expect(() => parseToolArgs('page_snapshot', { format: 'yaml' })).toThrow();
    expect(() => parseToolArgs('page_snapshot', { max_nodes: 39 })).toThrow();
    expect(() => parseToolArgs('page_snapshot', { max_nodes: 3001 })).toThrow();
  });

  it('keeps loose schemas loose and validates tab identifiers as strings', () => {
    expect(() => parseToolArgs('browser_wait', { anything: true })).not.toThrow();
    expect(() => parseToolArgs('tab_close', { tab_id: '7' })).not.toThrow();
    expect(() => parseToolArgs('tab_close', { tab_id: 7 })).toThrow();
  });
});

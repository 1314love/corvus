import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = process.cwd();
const snapshotPath = resolve(root, '..', 'tests', 'conformance', 'schema-snapshot.v1.7.0.json');
const tools = JSON.parse(readFileSync(snapshotPath, 'utf8'));

if (!Array.isArray(tools) || tools.length !== 62) {
  throw new Error(
    'expected 62 tools in the schema snapshot, found ' + (Array.isArray(tools) ? tools.length : 'non-array'),
  );
}

function literal(value) {
  return JSON.stringify(value);
}

function tsType(schema) {
  if (!schema || typeof schema !== 'object') return 'unknown';
  if (schema.const !== undefined) return literal(schema.const);
  if (Array.isArray(schema.enum)) return schema.enum.map(literal).join(' | ');
  if (Array.isArray(schema.anyOf)) return schema.anyOf.map(tsType).join(' | ');
  if (Array.isArray(schema.oneOf)) return schema.oneOf.map(tsType).join(' | ');
  if (Array.isArray(schema.type)) return schema.type.map((type) => tsType({ ...schema, type })).join(' | ');

  switch (schema.type) {
    case 'string':
      return 'string';
    case 'integer':
    case 'number':
      return 'number';
    case 'boolean':
      return 'boolean';
    case 'null':
      return 'null';
    case 'array':
      return 'Array<' + tsType(schema.items || {}) + '>';
    case 'object': {
      const properties = schema.properties && typeof schema.properties === 'object' ? schema.properties : null;
      if (properties) {
        const required = new Set(Array.isArray(schema.required) ? schema.required : []);
        const members = Object.entries(properties).map(([name, prop]) => {
          const safe = /^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name) ? name : literal(name);
          return safe + (required.has(name) ? '' : '?') + ': ' + tsType(prop);
        });
        if (schema.additionalProperties && typeof schema.additionalProperties === 'object') {
          members.push('[key: string]: unknown');
        } else if (schema.additionalProperties === true) {
          members.push('[key: string]: unknown');
        }
        return members.length ? '{ ' + members.join('; ') + ' }' : 'Record<string, never>';
      }
      if (schema.additionalProperties && typeof schema.additionalProperties === 'object') {
        return 'Record<string, ' + tsType(schema.additionalProperties) + '>';
      }
      return schema.additionalProperties === false ? 'Record<string, never>' : 'Record<string, unknown>';
    }
    default:
      return 'unknown';
  }
}

function zodType(schema) {
  if (!schema || typeof schema !== 'object') return 'z.unknown()';
  if (schema.const !== undefined) return 'z.literal(' + literal(schema.const) + ')';
  if (Array.isArray(schema.enum)) return union(schema.enum.map((value) => 'z.literal(' + literal(value) + ')'));
  if (Array.isArray(schema.anyOf)) return union(schema.anyOf.map(zodType));
  if (Array.isArray(schema.oneOf)) return union(schema.oneOf.map(zodType));
  if (Array.isArray(schema.type)) return union(schema.type.map((type) => zodType({ ...schema, type })));

  switch (schema.type) {
    case 'string': {
      let value = 'z.string()';
      if (Number.isInteger(schema.minLength)) value += '.min(' + schema.minLength + ')';
      if (Number.isInteger(schema.maxLength)) value += '.max(' + schema.maxLength + ')';
      if (typeof schema.pattern === 'string') value += '.regex(new RegExp(' + literal(schema.pattern) + '))';
      return value;
    }
    case 'integer':
    case 'number': {
      let value = schema.type === 'integer' ? 'z.number().int()' : 'z.number()';
      if (typeof schema.minimum === 'number') value += '.min(' + schema.minimum + ')';
      if (typeof schema.maximum === 'number') value += '.max(' + schema.maximum + ')';
      return value;
    }
    case 'boolean':
      return 'z.boolean()';
    case 'null':
      return 'z.null()';
    case 'array': {
      let value = 'z.array(' + zodType(schema.items || {}) + ')';
      if (Number.isInteger(schema.minItems)) value += '.min(' + schema.minItems + ')';
      if (Number.isInteger(schema.maxItems)) value += '.max(' + schema.maxItems + ')';
      return value;
    }
    case 'object': {
      const properties = schema.properties && typeof schema.properties === 'object' ? schema.properties : null;
      if (properties) {
        const required = new Set(Array.isArray(schema.required) ? schema.required : []);
        const members = Object.entries(properties).map(([name, prop]) => {
          let value = zodType(prop);
          if (prop && Object.prototype.hasOwnProperty.call(prop, 'default'))
            value += '.default(' + literal(prop.default) + ')';
          if (!required.has(name)) value += '.optional()';
          return literal(name) + ': ' + value;
        });
        let object = 'z.object({ ' + members.join(', ') + ' })';
        if (schema.additionalProperties === false) object += '.strict()';
        else if (schema.additionalProperties === true) object += '.passthrough()';
        return object;
      }
      if (schema.additionalProperties && typeof schema.additionalProperties === 'object') {
        return 'z.record(' + zodType(schema.additionalProperties) + ')';
      }
      return schema.additionalProperties === false ? 'z.object({}).strict()' : 'z.record(z.unknown())';
    }
    default:
      return 'z.unknown()';
  }
}

function union(parts) {
  if (parts.length === 0) return 'z.unknown()';
  return parts.slice(1).reduce((left, right) => left + '.or(' + right + ')', parts[0]);
}

const names = tools.map((tool) => tool.name);
const lines = [];
lines.push('// Generated by scripts/generate-tool-contracts.mjs from tests/conformance/schema-snapshot.v1.7.0.json.');
lines.push('// Do not edit this file by hand.');
lines.push("import { z } from 'zod';");
lines.push('');
lines.push('export const TOOL_NAMES = ' + JSON.stringify(names, null, 2) + ' as const;');
lines.push('');
lines.push('export type ToolName = (typeof TOOL_NAMES)[number];');
lines.push('');
lines.push('export interface ToolArgsByName {');
for (const tool of tools) {
  lines.push('  /** ' + tool.name + ' */');
  lines.push('  ' + literal(tool.name) + ': ' + tsType(tool.inputSchema) + ';');
}
lines.push('}');
lines.push('');
lines.push('export type ToolArgs<K extends ToolName> = ToolArgsByName[K];');
lines.push('');
lines.push('export const ToolArgsSchemas = {');
for (const tool of tools) {
  lines.push('  ' + literal(tool.name) + ': ' + zodType(tool.inputSchema) + ',');
}
lines.push('} satisfies Record<ToolName, z.ZodTypeAny>;');
lines.push('');
lines.push('export type ToolArgsInput<K extends ToolName> = z.input<(typeof ToolArgsSchemas)[K]>;');
lines.push('export type ToolArgsParsed<K extends ToolName> = z.output<(typeof ToolArgsSchemas)[K]>;');
lines.push('');
lines.push('export function parseToolArgs<K extends ToolName>(name: K, value: unknown): ToolArgsParsed<K> {');
lines.push('  return ToolArgsSchemas[name].parse(value) as ToolArgsParsed<K>;');
lines.push('}');
lines.push('');

const output = lines.join('\n');
if (process.argv.includes('--stdout')) {
  process.stdout.write(output);
} else {
  const target = resolve(root, 'src', 'shared', 'contracts', 'tools.generated.ts');
  writeFileSync(target, output, 'utf8');
  process.stdout.write('generated ' + target + ' from ' + snapshotPath + ' (' + output.length + ' bytes)\n');
}

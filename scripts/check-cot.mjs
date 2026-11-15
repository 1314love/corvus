#!/usr/bin/env node
/**
 * @fileoverview Gate for reasoning transcripts in tracked files and commit messages.
 *
 * docs/LANGUAGE.md section 5 and docs/PROCESS.md section 5 forbid reasoning
 * transcripts and deliberation. This script is the mechanical check: it scans
 * every tracked text file, or the commits in a git range, and fails on a hit.
 * A line that quotes the rule itself carries \"cot-gate: allow\".
 *
 * Usage: node scripts/check-cot.mjs [--commits <range>] [--self-test]
 * Exit:  0 clean, 1 findings, 2 usage or git failure.
 */

import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const ALLOW = 'cot-gate: allow';

const RULES = [
  { id: 'think-tag', note: 'model reasoning tag', pattern: /<\/?(?:think|thinking|reasoning)\b/i },
  { id: 'reasoning-content', note: 'model reasoning field', pattern: /reasoning_content/ },  // cot-gate: allow
  { id: 'chat-transcript', note: 'pasted chat transcript', pattern: /"role"\s*:\s*"assistant"/ },
  { id: 'model-disclosure', note: 'model self-description', pattern: /\bas an ai\b|\bas a language model\b/i },
  { id: 'deliberation', note: 'deliberation opener', pattern: /\blet me\b|\bi'll\b|\bi think\b|\bi was thinking\b|让我|我认为|我们先|首先我|接下来我/i },  // cot-gate: allow
  { id: 'cot-phrase', note: 'chain-of-thought wording', pattern: /chain[- ]of[- ]thought|思考过程|推理过程/i },  // cot-gate: allow
];

/** Returns the rules a single line violates; an allow marker clears the line. */
function scanLine(line) {
  if (line.includes(ALLOW)) return [];
  return RULES.filter((rule) => rule.pattern.test(line));
}

function formatHit(id, note, where, line) {
  return where + ': ' + id + ' (' + note + '): ' + line.trim().slice(0, 120);
}

function scanFiles() {
  const hits = [];
  const tracked = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8' }).split('\u0000').filter(Boolean);
  for (const file of tracked) {
    let buffer;
    try {
      buffer = readFileSync(file);
    } catch {
      continue;
    }
    if (buffer.includes(0)) continue;
    const lines = buffer.toString('utf8').split(/\r?\n/);
    for (let index = 0; index < lines.length; index += 1) {
      for (const rule of scanLine(lines[index])) hits.push(formatHit(rule.id, rule.note, file + ':' + (index + 1), lines[index]));
    }
  }
  return { hits, scanned: tracked.length };
}

function scanCommits(range) {
  const hits = [];
  let raw;
  try {
    raw = execFileSync('git', ['log', '--format=%H%x00%B%x00', range], { encoding: 'utf8' });
  } catch {
    throw new Error('cannot read git range ' + range);
  }
  const parts = raw.split('\u0000');
  let count = 0;
  for (let index = 0; index + 1 < parts.length; index += 2) {
    const sha = parts[index].trim().slice(0, 12);
    const body = parts[index + 1] || '';
    count += 1;
    const lines = body.split(/\r?\n/);
    for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
      for (const rule of scanLine(lines[lineIndex])) {
        hits.push(formatHit(rule.id, rule.note, 'commit ' + sha + ' line ' + (lineIndex + 1), lines[lineIndex]));
      }
    }
  }
  return { hits, scanned: count };
}

function selfTest() {
  const cases = [
    ['<think>private reasoning</think>', 'think-tag'],  // cot-gate: allow
    ['{"reasoning_content":"let me look"}', 'reasoning-content'],  // cot-gate: allow
    ['{"role": "assistant", "content": "done"}', 'chat-transcript'],  // cot-gate: allow
    ['As an AI language model I cannot verify that.', 'model-disclosure'],  // cot-gate: allow
    ['Let me check the failing test first.', 'deliberation'],  // cot-gate: allow
    ['我们先把问题拆开看。', 'deliberation'],  // cot-gate: allow
    ['This note explains the chain of thought behind the change.', 'cot-phrase'],  // cot-gate: allow
    ['A line that quotes the rule carries a marker. <!-- cot-gate: allow -->', null],
    ['No chain of thought. <!-- cot-gate: allow -->', null],
    ['The daemon writes SHA256SUMS; see docs/LANGUAGE.md.', null],
  ];
  const failures = [];
  for (const [text, expected] of cases) {
    const found = scanLine(text).map((rule) => rule.id);
    if (expected === null && found.length > 0) failures.push('false positive on: ' + text);
    if (expected !== null && !found.includes(expected)) failures.push('missed ' + expected + ' in: ' + text);
  }
  if (failures.length > 0) {
    console.error('self-test failed');
    for (const failure of failures) console.error('  ' + failure);
    return 2;
  }
  console.log('self-test: ' + cases.length + ' cases pass');
  return 0;
}

const args = process.argv.slice(2);
let range = null;
let selfTestOnly = false;
let status = 0;

for (let index = 0; index < args.length; index += 1) {
  const arg = args[index];
  if (arg === '--self-test') {
    status = selfTest();
    selfTestOnly = true;
  } else if (arg === '--commits') {
    range = args[index + 1];
    index += 1;
    if (!range) {
      console.error('--commits needs a git range');
      status = 2;
    }
  } else {
    console.error('unknown argument: ' + arg);
    status = 2;
  }
}

if (status !== 0) process.exit(status);
if (selfTestOnly && !range) process.exit(0);

const hits = [];
let summary = '';
try {
  if (range) {
    const commits = scanCommits(range);
    hits.push(...commits.hits);
    summary = 'checked ' + commits.scanned + ' commits in ' + range;
  } else {
    const files = scanFiles();
    hits.push(...files.hits);
    summary = 'checked ' + files.scanned + ' tracked files';
  }
} catch (error) {
  console.error('error: ' + error.message);
  process.exit(2);
}

for (const hit of hits) console.log(hit);
console.log(summary + ': ' + hits.length + ' findings');
process.exit(hits.length > 0 ? 1 : 0);

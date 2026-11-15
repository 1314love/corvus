// Test artifact location policy.
//
// Every artifact a test produces (MCP response dumps, JSONL traces, screenshots,
// evidence JSON, temporary SQLite files) is written below <tmp>/corvus-tests,
// never inside the repository and never into the directory a command started in.
//
// <tmp> is CORVUS_TEST_TMP when set, otherwise os.tmpdir(), which is %TEMP% on
// Windows. Set CORVUS_TEST_RUN to group the artifacts of one run across processes.
//
// CLI:
//   node tests/lib/tmp.mjs dir [run]          print the run directory
//   node tests/lib/tmp.mjs path <file> [run]  print a file path inside that directory
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";

const FOLDER = "corvus-tests";
const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

function checkedName(value, label) {
  if (typeof value !== "string" || !NAME.test(value)) {
    throw new Error(label + " must be a plain file name: " + JSON.stringify(value));
  }
  return value;
}

function stamp() {
  const now = new Date();
  const pad = (n) => String(n).padStart(2, "0");
  return (
    String(now.getFullYear()) +
    pad(now.getMonth() + 1) +
    pad(now.getDate()) +
    "-" +
    pad(now.getHours()) +
    pad(now.getMinutes()) +
    pad(now.getSeconds())
  );
}

// Creates <tmp>/corvus-tests and returns it.
export function testTmpRoot() {
  const base = process.env.CORVUS_TEST_TMP || tmpdir();
  const root = join(base, FOLDER);
  mkdirSync(root, { recursive: true });
  return root;
}

// Creates <tmp>/corvus-tests/<run> and returns it.
export function testRunDir(name) {
  const run = checkedName(name || process.env.CORVUS_TEST_RUN || "manual-" + stamp(), "run name");
  const dir = join(testTmpRoot(), run);
  mkdirSync(dir, { recursive: true });
  return dir;
}

// Creates the parent of <tmp>/corvus-tests/<run>/<file> and returns the file path.
export function testArtifact(file, run) {
  const target = join(testRunDir(run), checkedName(file, "file name"));
  mkdirSync(dirname(target), { recursive: true });
  return target;
}

const invoked = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : "";
if (import.meta.url === invoked) {
  const [mode, ...rest] = process.argv.slice(2);
  try {
    if (mode === "dir") {
      console.log(testRunDir(rest[0]));
    } else if (mode === "path" && rest[0]) {
      console.log(testArtifact(rest[0], rest[1]));
    } else {
      console.error("usage: tmp.mjs dir [run] | tmp.mjs path <file> [run]");
      process.exit(2);
    }
  } catch (error) {
    console.error("tmp.mjs: " + error.message);
    process.exit(2);
  }
}

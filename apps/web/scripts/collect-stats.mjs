// 从仓库里数出展示页要用的数字，写进 src/data/stats.generated.json。
// 只读仓库，不改任何别的文件。数字变了就重新跑一遍：pnpm stats。
//
// Counts the numbers the showcase page displays and writes them to
// src/data/stats.generated.json. It only reads the repository. Run `pnpm stats` to refresh.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..", "..");
const outputPath = join(here, "..", "src", "data", "stats.generated.json");

const SKIPPED_DIRS = new Set(["target", "node_modules", ".git", "dist", "gen"]);

/** 递归列出某个扩展名的文件。 / Lists files with one extension, recursively. */
function listFiles(dir, extension, found = []) {
  for (const name of readdirSync(dir)) {
    if (SKIPPED_DIRS.has(name)) {
      continue;
    }
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      listFiles(path, extension, found);
    } else if (name.endsWith(extension)) {
      found.push(path);
    }
  }
  return found;
}

function countLines(text) {
  if (text.length === 0) {
    return 0;
  }
  const breaks = text.split("\n").length - 1;
  return text.endsWith("\n") ? breaks : breaks + 1;
}

function git(...args) {
  return execFileSync("git", args, { cwd: repoRoot, encoding: "utf8" }).trim();
}

function listPackages(parent) {
  return readdirSync(join(repoRoot, parent))
    .filter((name) => statSync(join(repoRoot, parent, name)).isDirectory())
    .sort();
}

/**
 * 统计一个包的 Rust 代码。generated 目录下的文件是从官方服务端导出的数据表，单独记。
 *
 * Counts one package's Rust code. Files under a `generated` directory are data tables
 * exported from the official server, so they are counted separately.
 */
function measurePackage(parent, name) {
  const root = join(repoRoot, parent, name);
  let handwrittenLines = 0;
  let generatedLines = 0;
  let files = 0;
  let tests = 0;
  for (const path of listFiles(root, ".rs")) {
    const text = readFileSync(path, "utf8");
    const lines = countLines(text);
    files += 1;
    tests += (text.match(/#\[(?:tokio::)?test\b/g) ?? []).length;
    if (relative(root, path).split(sep).includes("generated")) {
      generatedLines += lines;
    } else {
      handwrittenLines += lines;
    }
  }
  return { name, path: `${parent}/${name}`, lines: handwrittenLines, generatedLines, files, tests };
}

const crates = listPackages("crates").map((name) => measurePackage("crates", name));
const tools = listPackages("tools").map((name) => measurePackage("tools", name));
const packages = [...crates, ...tools];

const sum = (items, pick) => items.reduce((total, item) => total + pick(item), 0);

const javaLines = sum(listFiles(join(repoRoot, "tools"), ".java"), (path) =>
  countLines(readFileSync(path, "utf8")),
);

const stats = {
  commit: git("rev-parse", "--short", "HEAD"),
  commits: Number(git("rev-list", "--count", "HEAD")),
  firstCommitDate: git("log", "--reverse", "--format=%ad", "--date=short").split("\n")[0],
  lastCommitDate: git("log", "-1", "--format=%ad", "--date=short"),
  rustLines: sum(packages, (item) => item.lines),
  generatedRustLines: sum(packages, (item) => item.generatedLines),
  rustFiles: sum(packages, (item) => item.files),
  tests: sum(packages, (item) => item.tests),
  oracleJavaLines: javaLines,
  crates,
  tools,
};

writeFileSync(outputPath, `${JSON.stringify(stats, null, 2)}\n`);
console.log(
  `${stats.commit}: ${stats.commits} commits, ${stats.rustLines} Rust lines ` +
    `(+${stats.generatedRustLines} generated), ${stats.tests} tests, ` +
    `${crates.length} crates, ${tools.length} tools`,
);

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { makeGeneratedEntity } from "./demo-data.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function readArg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  if (index === -1) return fallback;
  return process.argv[index + 1] ?? fallback;
}

const workdirArg = readArg("workdir", "demo/workdir");
const stageArg = readArg("stage", "incoming");
const count = Number.parseInt(readArg("count", "1000"), 10);
const seed = Number.parseInt(readArg("seed", "42"), 10);

if (!Number.isFinite(count) || count < 1) {
  throw new Error("--count must be a positive integer");
}

const workdir = path.resolve(repoRoot, workdirArg);
const stageRelative = stageArg.includes("/") || stageArg.includes("\\")
  ? stageArg
  : path.join("stages", stageArg);
const outputDir = path.resolve(workdir, stageRelative);

if (!outputDir.startsWith(workdir)) {
  throw new Error(`Refusing to write outside workdir: ${outputDir}`);
}

fs.mkdirSync(outputDir, { recursive: true });

for (let index = 0; index < count; index += 1) {
  const entity = makeGeneratedEntity(index, seed);
  const filePath = path.join(outputDir, `${entity.id}.json`);
  fs.writeFileSync(filePath, `${JSON.stringify(entity, null, 2)}\n`, "utf8");
}

console.log(`Generated ${count} demo JSON files in ${outputDir}`);

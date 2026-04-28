import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { demoItems, demoPipelineYaml, makeDemoEntity } from "./demo-data.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const workdir = path.join(repoRoot, "demo", "workdir");
const stagesDir = path.join(workdir, "stages");
const incomingDir = path.join(stagesDir, "incoming");
const outputDir = path.join(stagesDir, "n8n_output");
const reviewDir = path.join(stagesDir, "review");
const invalidDir = path.join(stagesDir, "invalid_samples");
const logsDir = path.join(workdir, "logs");

function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

function emptyDir(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
  ensureDir(dir);
}

function writeJson(filePath, value) {
  fs.writeFileSync(`${filePath}.tmp`, `${JSON.stringify(value, null, 2)}\n`, "utf8");
  fs.renameSync(`${filePath}.tmp`, filePath);
}

ensureDir(workdir);
ensureDir(stagesDir);
emptyDir(incomingDir);
emptyDir(outputDir);
emptyDir(reviewDir);
emptyDir(invalidDir);
ensureDir(logsDir);
fs.writeFileSync(path.join(outputDir, ".gitkeep"), "", "utf8");
fs.writeFileSync(path.join(reviewDir, ".gitkeep"), "", "utf8");
fs.writeFileSync(path.join(logsDir, ".gitkeep"), "", "utf8");

fs.rmSync(path.join(workdir, "app.db"), { force: true });
fs.writeFileSync(path.join(workdir, "pipeline.yaml"), demoPipelineYaml, "utf8");

demoItems.forEach(([id, entityName, parentName, grandparentName], index) => {
  writeJson(
    path.join(incomingDir, `${id}.json`),
    makeDemoEntity(id, entityName, parentName, grandparentName, index),
  );
});

fs.writeFileSync(path.join(invalidDir, "invalid-syntax.json"), "{ this is not valid json\n", "utf8");
writeJson(path.join(invalidDir, "missing-id.json"), {
  current_stage: "semantic_split",
  next_stage: "review",
  status: "pending",
  payload: { entity_name: "invalid sample without id" },
  meta: { source: "demo-invalid" },
});
writeJson(path.join(invalidDir, "missing-payload.json"), {
  id: "invalid-missing-payload",
  current_stage: "semantic_split",
  next_stage: "review",
  status: "pending",
  meta: { source: "demo-invalid" },
});

console.log(`Demo workdir reset: ${workdir}`);
console.log(`Input files: ${demoItems.length}`);

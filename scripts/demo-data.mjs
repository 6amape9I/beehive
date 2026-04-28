export const demoPipelineYaml = `project:
  name: beehive-demo
  workdir: .

runtime:
  scan_interval_sec: 5
  max_parallel_tasks: 3
  stuck_task_timeout_sec: 120
  request_timeout_sec: 60
  file_stability_delay_ms: 300

stages:
  - id: semantic_split
    input_folder: stages/incoming
    output_folder: stages/n8n_output
    workflow_url: https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
    max_attempts: 2
    retry_delay_sec: 5
    next_stage: review

  - id: review
    input_folder: stages/n8n_output
    output_folder: ""
    workflow_url: https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
    max_attempts: 2
    retry_delay_sec: 5
    next_stage: null
`;

export const demoItems = [
  ["demo-ceramic-001", "керамика", "материал", "материя"],
  ["demo-horizon-001", "горизонт", "граница", "пространство"],
  ["demo-castle-001", "замок", "здание", "сооружение"],
  ["demo-glass-001", "стекло", "материал", "вещество"],
  ["demo-bridge-001", "мост", "переход", "инфраструктура"],
  ["demo-river-001", "река", "водный поток", "ландшафт"],
  ["demo-archive-001", "архив", "хранилище", "информация"],
  ["demo-compass-001", "компас", "инструмент", "навигация"],
  ["demo-cloud-001", "облако", "атмосферное явление", "погода"],
  ["demo-signal-001", "сигнал", "сообщение", "коммуникация"],
];

export function makeDemoEntity(id, entityName, parentName, grandparentName, index = 0) {
  return {
    id,
    current_stage: "semantic_split",
    next_stage: "review",
    status: "pending",
    payload: {
      entity_name: entityName,
      source_parent_name: parentName,
      source_grandparent_name: grandparentName,
      source_entity_context: `Demo ontology sample ${index + 1}`,
      source_semantic_description: null,
      parent_name_candidate: null,
      grandparent_name_candidate: null,
      parent_name: "string | null",
      grandparent_name: "string | null",
      entity_weight: null,
      entity_fullness: null,
      relation_id: null,
      relation_weight: null,
      relation_truth: null,
      strength: null,
      source_entity_id: null,
      current_languages: ["ru"],
      ready: null,
      representations: [],
    },
    meta: {
      source: "demo",
      created_at: "2026-04-27T00:00:00Z",
    },
  };
}

export function makeGeneratedEntity(index, seed = 42) {
  const serial = String(index + 1).padStart(5, "0");
  const concepts = [
    "керамика",
    "горизонт",
    "замок",
    "стекло",
    "мост",
    "река",
    "архив",
    "компас",
    "облако",
    "сигнал",
  ];
  const parents = [
    "материал",
    "граница",
    "здание",
    "вещество",
    "переход",
    "ландшафт",
    "хранилище",
    "инструмент",
    "погода",
    "сообщение",
  ];
  const slot = (index + seed) % concepts.length;
  const entity = makeDemoEntity(
    `generated-demo-${seed}-${serial}`,
    `${concepts[slot]} ${serial}`,
    parents[slot],
    "generated-demo",
    index,
  );
  entity.meta.source = "demo-generator";
  return entity;
}

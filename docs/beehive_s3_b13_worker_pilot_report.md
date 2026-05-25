# B13 Worker Pilot Report

## Pilot Status

Status: blocked in this turn.

B13 did not run a real worker pilot because there is no explicitly prepared test workspace with 20-100 safe files and a known safe mock or real n8n endpoint in the current request.

The production-scale workspace `itg_documents` was not used for execution. B13 code does not add subset-limited worker processing for `itg_documents`, so using it for a worker pilot would violate the instruction.

## Required Pilot Inputs

Before a real pilot, provide or create a safe test workspace with:

- workspace_id: a non-production id, for example `test_worker_pilot`
- 20-100 test artifacts
- pipeline stages with safe workflow URLs
- explicit confirmation whether workflow calls should hit a mock endpoint or real n8n
- runtime pool config with default concurrency 3-5 and local LLM concurrency 1

## Pilot Fields

workspace_id: not run

worker env: not run

pool config: not run

number of tasks processed: 0

success/retry/failed/blocked: 0/0/0/0

local_llm max observed active leases: not observed

default max observed active leases: not observed

throughput: not measured

errors: pilot blocked by missing safe test workspace and endpoint selection

whether S3/n8n real calls were made: no

## itg_documents

`itg_documents` was not modified, reset, archived, cleaned, imported, or used for smoke execution.

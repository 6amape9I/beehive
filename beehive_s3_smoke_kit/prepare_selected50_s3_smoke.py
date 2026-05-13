#!/usr/bin/env python3
"""Prepare S3 smoke-test source objects.

This script does not contact S3. It creates:
  - smoke_source_objects/*.json
  - smoke_source_manifest.json
  - upload_s3_smoke_objects.sh

The upload script expects env vars:
  S3_HOST, S3_REGION, S3_KEY, S3_SEC_KEY, S3_BUCKET_NAME
Optional:
  BEEHIVE_SMOKE_PREFIX, default beehive-smoke/test_workflow
"""
from __future__ import annotations

import argparse
import json
import re
import stat
import zipfile
from pathlib import Path
from typing import Any

DEFAULT_PREFIX = "beehive-smoke/test_workflow"
SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_FIXTURES_DIR = SCRIPT_DIR / "fixtures" / "minimal_raw"


def safe_slug(value: str, max_len: int = 80) -> str:
    value = value.strip().lower()
    value = value.replace("ё", "е")
    value = re.sub(r"[^a-zа-я0-9]+", "-", value, flags=re.IGNORECASE).strip("-")
    if not value:
        value = "entity"
    return value[:max_len].strip("-") or "entity"


def first_text_blocks(article: dict[str, Any], max_blocks: int = 6) -> list[dict[str, Any]]:
    blocks = article.get("content", {}).get("blocks", [])
    out: list[dict[str, Any]] = []
    for block in blocks:
        if not isinstance(block, dict):
            continue
        block_type = block.get("type")
        data = block.get("data", {}) if isinstance(block.get("data"), dict) else {}
        text = data.get("text")
        if block_type in {"paragraph", "header"} and isinstance(text, str) and text.strip():
            out.append({"type": block_type, "text": text.strip()})
        elif block_type == "list" and isinstance(data.get("items"), list):
            items = [str(item).strip() for item in data["items"] if str(item).strip()]
            if items:
                out.append({"type": "list", "items": items[:5]})
        if len(out) >= max_blocks:
            break
    return out


def load_raw_docs(zip_path: Path) -> list[tuple[str, dict[str, Any]]]:
    docs: list[tuple[str, dict[str, Any]]] = []
    with zipfile.ZipFile(zip_path) as zf:
        names = sorted(
            name for name in zf.namelist()
            if name.startswith("selected_50_for_n8n/raw/") and name.endswith(".json")
        )
        for name in names:
            with zf.open(name) as fh:
                docs.append((name, json.loads(fh.read().decode("utf-8"))))
    return docs


def load_fixture_docs(fixtures_dir: Path) -> list[tuple[str, dict[str, Any]]]:
    docs: list[tuple[str, dict[str, Any]]] = []
    for path in sorted(fixtures_dir.glob("*.json")):
        docs.append((str(path.relative_to(fixtures_dir)), json.loads(path.read_text(encoding="utf-8"))))
    return docs


def resolve_input_path(value: str) -> Path:
    path = Path(value)
    if path.is_absolute() or path.exists():
        return path.resolve()
    return (SCRIPT_DIR / path).resolve()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--zip", default=None, help="Optional external source zip with selected_50_for_n8n/raw/*.json")
    parser.add_argument("--fixtures", default=str(DEFAULT_FIXTURES_DIR), help="Fixture directory used when --zip is omitted")
    parser.add_argument("--out", default="s3_smoke_dataset", help="Output directory")
    parser.add_argument("--prefix", default=DEFAULT_PREFIX, help="Default S3 prefix used in generated manifest/upload script")
    parser.add_argument("--limit", type=int, default=3, help="How many docs to prepare")
    args = parser.parse_args()

    out_dir = Path(args.out).resolve()
    source_dir = out_dir / "smoke_source_objects"
    source_dir.mkdir(parents=True, exist_ok=True)

    if args.zip:
        docs = load_raw_docs(resolve_input_path(args.zip))[: args.limit]
    else:
        docs = load_fixture_docs(resolve_input_path(args.fixtures))[: args.limit]
    if not docs:
        raise SystemExit("No smoke source documents found.")

    manifest: dict[str, Any] = {
        "schema": "beehive.s3_smoke_seed_manifest.v1",
        "default_prefix": args.prefix.strip("/"),
        "source_prefix": f"{args.prefix.strip('/')}/raw",
        "target_prefix": f"{args.prefix.strip('/')}/processed",
        "count": len(docs),
        "artifacts": [],
    }

    for index, (zip_name, article) in enumerate(docs, start=1):
        canonical = str(article.get("canonical_tag_ru") or article.get("tag_id") or f"entity-{index}")
        entity_type = str(article.get("entity_type") or "unknown")
        tag_id = str(article.get("tag_id") or f"tag_{index:03d}")
        entity_id = f"smoke_entity_{index:03d}"
        artifact_id = f"smoke_source_artifact_{index:03d}"
        file_name = f"{entity_id}__{safe_slug(canonical)}.json"
        key = f"{args.prefix.strip('/')}/raw/{file_name}"
        source_object = {
            "schema": "beehive.s3_smoke_source.v1",
            "entity_id": entity_id,
            "artifact_id": artifact_id,
            "canonical_tag_ru": canonical,
            "canonical_tag_latin": article.get("canonical_tag_latin"),
            "entity_type": entity_type,
            "tag_id": tag_id,
            "article_status": article.get("article_status"),
            "needs_review_before_publication": article.get("needs_review_before_publication"),
            "source_zip_entry": zip_name,
            "preview_blocks": first_text_blocks(article),
            "raw_article": article,
        }
        path = source_dir / file_name
        path.write_text(json.dumps(source_object, ensure_ascii=False, indent=2), encoding="utf-8")
        manifest["artifacts"].append({
            "index": index,
            "entity_id": entity_id,
            "artifact_id": artifact_id,
            "stage_id": "smoke_source",
            "bucket_env": "S3_BUCKET_NAME",
            "key": key,
            "local_file": str(path.relative_to(out_dir)),
            "canonical_tag_ru": canonical,
            "entity_type": entity_type,
            "tag_id": tag_id,
        })

    (out_dir / "smoke_source_manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    upload_script = out_dir / "upload_s3_smoke_objects.sh"
    upload_lines = [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        ': "${S3_HOST:?Set S3_HOST, e.g. s3.ru-1.storage.selcloud.ru}"',
        ': "${S3_REGION:?Set S3_REGION, e.g. ru-1}"',
        ': "${S3_KEY:?Set S3_KEY}"',
        ': "${S3_SEC_KEY:?Set S3_SEC_KEY}"',
        ': "${S3_BUCKET_NAME:?Set S3_BUCKET_NAME, e.g. steos-s3-data}"',
        f'BEEHIVE_SMOKE_PREFIX="${{BEEHIVE_SMOKE_PREFIX:-{args.prefix.strip("/")}}}"',
        'ENDPOINT="${S3_ENDPOINT:-$S3_HOST}"',
        'case "$ENDPOINT" in http://*|https://*) ;; *) ENDPOINT="https://$ENDPOINT" ;; esac',
        'export AWS_ACCESS_KEY_ID="$S3_KEY"',
        'export AWS_SECRET_ACCESS_KEY="$S3_SEC_KEY"',
        'export AWS_REGION="$S3_REGION"',
        'SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"',
        'echo "Uploading smoke source objects to s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/raw/"',
    ]
    for artifact in manifest["artifacts"]:
        local_file = artifact["local_file"]
        key_template = artifact["key"].replace(args.prefix.strip("/"), "${BEEHIVE_SMOKE_PREFIX}", 1)
        metadata = (
            f"beehive-entity-id={artifact['entity_id']},"
            f"beehive-artifact-id={artifact['artifact_id']},"
            f"beehive-stage-id=smoke_source"
        )
        upload_lines.extend([
            f'aws s3api put-object --endpoint-url "$ENDPOINT" --bucket "$S3_BUCKET_NAME" --key "{key_template}" --body "$SCRIPT_DIR/{local_file}" --metadata "{metadata}" >/dev/null',
        ])
    upload_lines.extend([
        'echo "Done. Verify with:"',
        'echo "aws s3 ls --endpoint-url \"$ENDPOINT\" s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/raw/"',
    ])
    upload_script.write_text("\n".join(upload_lines) + "\n", encoding="utf-8")
    upload_script.chmod(upload_script.stat().st_mode | stat.S_IEXEC)

    print(f"Prepared {len(docs)} smoke objects in {out_dir}")
    print(f"Manifest: {out_dir / 'smoke_source_manifest.json'}")
    print(f"Upload script: {upload_script}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

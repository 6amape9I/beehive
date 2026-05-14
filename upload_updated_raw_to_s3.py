#!/usr/bin/env python3
"""
Prepare and upload updated selected_50 JSON documents to a Beehive S3 raw prefix.

This script:
1) extracts for_n8n_updated_selected_50_mark_down_text.zip;
2) creates prepared JSON files with raw_text=mark_down_text and source_name fields;
3) uploads every object with per-object Beehive metadata required by S3 reconciliation.

Requirements:
  pip install boto3

Environment:
  S3_HOST=s3.ru-1.storage.selcloud.ru
  S3_REGION=ru-1
  S3_KEY=...
  S3_SEC_KEY=...
  S3_BUCKET_NAME=steos-s3-data

Example:
  python upload_updated_raw_to_s3.py \
    --zip for_n8n_updated_selected_50_mark_down_text.zip \
    --prefix beehive-smoke/test_workflow/raw \
    --stage-id smoke_source \
    --clean --dry-run

  python upload_updated_raw_to_s3.py \
    --zip for_n8n_updated_selected_50_mark_down_text.zip \
    --prefix beehive-smoke/test_workflow/raw \
    --stage-id smoke_source \
    --clean
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import posixpath
import shutil
import sys
import zipfile
from pathlib import Path
from typing import Any

try:
    import boto3
    from botocore.config import Config
except Exception:  # pragma: no cover
    boto3 = None
    Config = None


def endpoint_url(host: str) -> str:
    host = host.strip()
    if host.startswith("http://") or host.startswith("https://"):
        return host
    return "https://" + host


def safe_ascii(value: str, fallback: str) -> str:
    value = (value or "").strip()
    out = []
    for ch in value:
        if ch.isascii() and (ch.isalnum() or ch in "._-"):
            out.append(ch)
        else:
            out.append("_")
    cleaned = "".join(out).strip("_")
    while "__" in cleaned:
        cleaned = cleaned.replace("__", "_")
    return cleaned[:180] or fallback


def load_json_objects(zip_path: Path) -> list[tuple[str, dict[str, Any]]]:
    rows: list[tuple[str, dict[str, Any]]] = []
    with zipfile.ZipFile(zip_path) as zf:
        for name in sorted(zf.namelist()):
            if name.endswith("/") or not name.lower().endswith(".json"):
                continue
            data = json.loads(zf.read(name))
            if not isinstance(data, dict):
                raise ValueError(f"{name}: root JSON is not an object")
            rows.append((name, data))
    if not rows:
        raise ValueError("No JSON files found in archive")
    return rows


def prepare_payloads(rows: list[tuple[str, dict[str, Any]]], out_dir: Path, stage_id: str, prefix: str) -> list[dict[str, Any]]:
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest: list[dict[str, Any]] = []
    for idx, (source_name, data) in enumerate(rows, start=1):
        tag_id = str(data.get("tag_id") or f"selected_{idx:03d}")
        entity_id = safe_ascii(tag_id, f"entity_{idx:03d}")
        artifact_id = safe_ascii(f"raw_{tag_id}", f"artifact_{idx:03d}")
        canonical = str(data.get("canonical_tag_ru") or data.get("canonical_tag_latin") or tag_id)
        original_filename = Path(source_name).name
        filename = f"{idx:03d}_{canonical}.json"
        key = posixpath.join(prefix.strip("/"), filename)

        payload = dict(data)
        # The first workflow still expects raw_text/source_name. Keep the new field too.
        if payload.get("mark_down_text") and not payload.get("raw_text"):
            payload["raw_text"] = payload["mark_down_text"]
        payload.setdefault("source_name", canonical)
        payload.setdefault("entity_id", entity_id)
        payload.setdefault("artifact_id", artifact_id)
        payload.setdefault("stage_id", stage_id)

        content = json.dumps(payload, ensure_ascii=False, indent=2).encode("utf-8")
        local_path = out_dir / filename
        local_path.write_bytes(content)
        manifest.append({
            "index": idx,
            "source_archive_path": source_name,
            "local_path": str(local_path),
            "bucket_key": key,
            "entity_id": entity_id,
            "artifact_id": artifact_id,
            "stage_id": stage_id,
            "tag_id": tag_id,
            "canonical_tag_ru": data.get("canonical_tag_ru"),
            "article_status": data.get("article_status"),
            "mark_down_text_chars": len(data.get("mark_down_text") or ""),
            "sha256": hashlib.sha256(content).hexdigest(),
            "metadata": {
                "beehive-entity-id": entity_id,
                "beehive-artifact-id": artifact_id,
                "beehive-stage-id": stage_id,
            },
        })
    (out_dir / "upload_manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--zip", required=True, type=Path)
    parser.add_argument("--prefix", required=True, help="S3 prefix, e.g. beehive-smoke/test_workflow/raw")
    parser.add_argument("--stage-id", required=True, help="Beehive stage id, e.g. smoke_source")
    parser.add_argument("--work-dir", default="prepared_updated_raw", type=Path)
    parser.add_argument("--clean", action="store_true", help="Delete existing objects under prefix before upload")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    bucket = os.environ.get("S3_BUCKET_NAME")
    missing = [name for name in ["S3_HOST", "S3_REGION", "S3_KEY", "S3_SEC_KEY", "S3_BUCKET_NAME"] if not os.environ.get(name)]
    if missing:
        print(f"Missing env variables: {', '.join(missing)}", file=sys.stderr)
        return 2
    if boto3 is None:
        print("boto3 is not installed. Run: python -m pip install boto3", file=sys.stderr)
        return 2

    rows = load_json_objects(args.zip)
    manifest = prepare_payloads(rows, args.work_dir, args.stage_id, args.prefix)
    print(f"Prepared {len(manifest)} JSON files into {args.work_dir}")

    verify_ssl = os.environ.get("S3_VERIFY_SSL", "true").lower() not in {"0", "false", "no"}
    ca_bundle = os.environ.get("AWS_CA_BUNDLE") or None
    verify_value = ca_bundle if ca_bundle else verify_ssl

    client = boto3.client(
        "s3",
        endpoint_url=endpoint_url(os.environ["S3_HOST"]),
        region_name=os.environ.get("S3_REGION") or "ru-1",
        aws_access_key_id=os.environ["S3_KEY"],
        aws_secret_access_key=os.environ["S3_SEC_KEY"],
        config=Config(s3={"addressing_style": "path"}),
        verify=verify_value,
    )

    prefix = args.prefix.strip("/")
    if args.clean:
        print(f"Cleaning s3://{bucket}/{prefix}/")
        if not args.dry_run:
            token = None
            while True:
                kwargs = {"Bucket": bucket, "Prefix": prefix + "/"}
                if token:
                    kwargs["ContinuationToken"] = token
                page = client.list_objects_v2(**kwargs)
                objects = [{"Key": item["Key"]} for item in page.get("Contents", [])]
                if objects:
                    client.delete_objects(Bucket=bucket, Delete={"Objects": objects, "Quiet": True})
                if not page.get("IsTruncated"):
                    break
                token = page.get("NextContinuationToken")

    for row in manifest:
        local_path = Path(row["local_path"])
        key = row["bucket_key"]
        metadata = row["metadata"]
        print(f"PUT s3://{bucket}/{key} metadata={metadata}")
        if args.dry_run:
            continue
        client.put_object(
            Bucket=bucket,
            Key=key,
            Body=local_path.read_bytes(),
            ContentType="application/json; charset=utf-8",
            Metadata=metadata,
        )
    print("Done.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

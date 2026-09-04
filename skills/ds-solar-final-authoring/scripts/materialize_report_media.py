#!/usr/bin/env python3
"""Create a presentation-only Markdown copy with bundle-local image links."""

from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path

MAX_BYTES = 16 * 1024 * 1024
IMAGE = re.compile(r"!\[([^\]]*)\]\(([^)]+)\)")


def bounded_text(path: Path, label: str) -> str:
    size = path.stat().st_size
    if size < 1 or size > MAX_BYTES:
        raise ValueError(f"{label} must be 1..{MAX_BYTES} bytes")
    return path.read_text(encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--final", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()

    final = args.final.resolve()
    manifest_path = args.manifest.resolve()
    destination = args.out.resolve()
    if destination in {final, manifest_path}:
        raise ValueError("output must be a separate rendering copy")
    if destination.exists():
        raise FileExistsError(f"refusing to replace {destination}")

    markdown = bounded_text(final, "final Markdown")
    manifest = json.loads(bounded_text(manifest_path, "media manifest"))
    if manifest.get("schema") != "ds.solar-report-media/v1":
        raise ValueError("unsupported Solar media manifest schema")
    images = manifest.get("images")
    if not isinstance(images, list):
        raise ValueError("media manifest images must be a list")
    mapping: dict[str, str] = {}
    for item in images:
        if not isinstance(item, dict):
            raise ValueError("media manifest image entry must be an object")
        reference = item.get("reference")
        bundle_path = item.get("bundle_path")
        if not isinstance(reference, str) or not reference.strip():
            raise ValueError("media reference is invalid")
        if not isinstance(bundle_path, str) or not bundle_path.startswith("media/"):
            raise ValueError(f"bundle path is invalid for {reference}")
        media = manifest_path.parent.joinpath(bundle_path).resolve()
        if manifest_path.parent.resolve() not in media.parents or not media.is_file():
            raise ValueError(f"bundle media is missing for {reference}")
        mapping[reference] = bundle_path

    seen: set[str] = set()

    def project(match: re.Match[str]) -> str:
        alt, raw_reference = match.groups()
        reference = raw_reference.strip()
        seen.add(reference)
        local = mapping.get(reference)
        if local is None:
            raise ValueError(f"final Markdown image is absent from manifest: {reference}")
        return f"![{alt}]({local})"

    rendered = IMAGE.sub(project, markdown)
    unused = sorted(set(mapping) - seen)
    if unused:
        raise ValueError(f"manifest contains images absent from final Markdown: {', '.join(unused)}")

    destination.parent.mkdir(parents=True, exist_ok=True)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    fd = os.open(destination, flags, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8", newline="") as output:
        output.write(rendered)
        output.flush()
        os.fsync(output.fileno())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

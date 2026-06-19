#!/usr/bin/env python3
"""Best-effort PDF visual asset extractor for research-os sessions.

This helper renders pages from a PDF and crops requested figure/table regions.
It intentionally does not claim perfect automatic layout parsing. Agents should
provide candidate page numbers and optional crop boxes when they can infer them
from captions or page inspection; otherwise the helper records a failed item.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

import fitz  # PyMuPDF
from PIL import Image


def slugify(value: str, fallback: str) -> str:
    value = value.lower()
    value = re.sub(r"[^a-z0-9]+", "_", value)
    value = value.strip("_")
    return value[:90] or fallback


def parse_box(value: str | None) -> tuple[float, float, float, float] | None:
    if not value:
        return None
    parts = [part.strip() for part in value.split(",")]
    if len(parts) != 4:
        raise ValueError("--box must be x0,y0,x1,y1")
    x0, y0, x1, y1 = [float(part) for part in parts]
    if x1 <= x0 or y1 <= y0:
        raise ValueError("--box requires x1>x0 and y1>y0")
    return x0, y0, x1, y1


def load_candidates(path: Path | None) -> list[dict[str, Any]]:
    if path is None:
        return []
    data = json.loads(path.read_text())
    if isinstance(data, list):
        return data
    if isinstance(data, dict):
        return data.get("assets", [])
    raise ValueError("candidate JSON must be an array or object with assets")


def normalize_box(
    box: tuple[float, float, float, float],
    page_width: float,
    page_height: float,
) -> fitz.Rect:
    x0, y0, x1, y1 = box
    if max(abs(x0), abs(y0), abs(x1), abs(y1)) <= 1.0:
        x0, x1 = x0 * page_width, x1 * page_width
        y0, y1 = y0 * page_height, y1 * page_height
    x0 = max(0.0, min(page_width, x0))
    x1 = max(0.0, min(page_width, x1))
    y0 = max(0.0, min(page_height, y0))
    y1 = max(0.0, min(page_height, y1))
    return fitz.Rect(x0, y0, x1, y1)


def render_clip(page: fitz.Page, rect: fitz.Rect, dpi: int, out_path: Path) -> None:
    matrix = fitz.Matrix(dpi / 72.0, dpi / 72.0)
    pix = page.get_pixmap(matrix=matrix, clip=rect, alpha=False)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    pix.save(out_path)
    with Image.open(out_path) as image:
        image.save(out_path, optimize=True)


def extract_asset(
    doc: fitz.Document,
    pdf_path: Path,
    output_dir: Path,
    source_slug: str,
    candidate: dict[str, Any],
    idx: int,
    dpi: int,
) -> dict[str, Any]:
    label = str(candidate.get("label") or f"asset_{idx}")
    asset_type = str(candidate.get("asset_type") or candidate.get("type") or "figure")
    page_number = int(candidate.get("page") or candidate.get("page_number") or 0)
    caption = str(candidate.get("caption") or "")
    rationale = str(candidate.get("rationale") or candidate.get("why_it_matters") or "")
    box_value = candidate.get("box") or candidate.get("bbox")

    result: dict[str, Any] = {
        "source_pdf": str(pdf_path),
        "asset_type": asset_type,
        "label": label,
        "caption": caption,
        "page": page_number,
        "extraction_status": "failed",
        "crop_method": "explicit_box",
        "asset_path": None,
        "rationale": rationale,
        "error": "",
    }

    if page_number < 1 or page_number > doc.page_count:
        result["error"] = f"page out of range: {page_number}"
        return result
    if box_value is None:
        result["extraction_status"] = "uncertain"
        result["error"] = "missing bbox; agent must provide explicit crop box"
        return result

    try:
        if isinstance(box_value, str):
            box = parse_box(box_value)
        else:
            box = tuple(float(part) for part in box_value)
        if box is None or len(box) != 4:
            raise ValueError("missing bbox")
        page = doc.load_page(page_number - 1)
        rect = normalize_box(box, page.rect.width, page.rect.height)
        if rect.width < 10 or rect.height < 10:
            raise ValueError("crop box too small")
        ext_dir = "tables" if asset_type == "table" else "figures"
        filename = f"{source_slug}__{slugify(label, f'asset_{idx}')}.png"
        out_path = output_dir / ext_dir / filename
        render_clip(page, rect, dpi, out_path)
        result["asset_path"] = str(out_path)
        result["extraction_status"] = "success"
        result["bbox"] = [rect.x0, rect.y0, rect.x1, rect.y1]
    except Exception as exc:  # noqa: BLE001 - CLI should report per-asset failure.
        result["error"] = str(exc)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", required=True, type=Path)
    parser.add_argument("--session", required=True, type=Path)
    parser.add_argument("--source-id", required=True)
    parser.add_argument("--source-slug")
    parser.add_argument("--candidates", type=Path)
    parser.add_argument("--label")
    parser.add_argument("--asset-type", choices=["figure", "table"], default="figure")
    parser.add_argument("--page", type=int)
    parser.add_argument("--box", help="Crop box x0,y0,x1,y1 in PDF points or 0..1 fractions")
    parser.add_argument("--caption", default="")
    parser.add_argument("--rationale", default="")
    parser.add_argument("--dpi", type=int, default=180)
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()

    source_slug = args.source_slug or slugify(args.source_id, "source")
    output_dir = args.session / "assets"
    candidates = load_candidates(args.candidates)
    if not candidates:
        candidates = [
            {
                "asset_type": args.asset_type,
                "label": args.label or args.asset_type,
                "page": args.page,
                "box": args.box,
                "caption": args.caption,
                "rationale": args.rationale,
            }
        ]

    try:
        doc = fitz.open(args.pdf)
    except Exception as exc:  # noqa: BLE001
        print(f"open pdf failed: {exc}", file=sys.stderr)
        return 2

    assets = [
        extract_asset(doc, args.pdf, output_dir, source_slug, candidate, idx + 1, args.dpi)
        for idx, candidate in enumerate(candidates)
    ]
    manifest = {
        "source_id": args.source_id,
        "source_slug": source_slug,
        "pdf_path": str(args.pdf),
        "assets": assets,
    }
    manifest_path = args.manifest or output_dir / f"{source_slug}__asset_manifest.json"
    manifest_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))
    return 0 if any(asset["extraction_status"] == "success" for asset in assets) else 1


if __name__ == "__main__":
    raise SystemExit(main())

---
source_id: iclr2026-fastavatar
title: "FastAvatar: Towards Unified and Fast 3D Avatar Reconstruction with Large Gaussian Reconstruction Transformers"
source_type: paper
date: 2026
raw_path: runs/run-1777908967/raw/sources/iclr2026-fastavatar.openreview.txt
parsed_path: runs/run-1777908967/parsed/sources/iclr2026-fastavatar.txt
url: https://openreview.net/forum?id=P7zBSCs4Xt
---

# FastAvatar: Towards Unified and Fast 3D Avatar Reconstruction with Large Gaussian Reconstruction Transformers

## Main Claims

- The source claims FastAvatar is a feed-forward 3D avatar reconstruction framework for single-image, multi-view, or monocular-video inputs.
- The source claims it reconstructs a 3D Gaussian Splatting avatar model within seconds.
- The source claims it handles variable-length inputs and can incrementally use additional observations to improve reconstruction quality.

## Evidence and Locations

- Parsed abstract: describes FastAvatar as taking single image, multi-view observations, or monocular video and reconstructing a 3D Gaussian Splatting avatar model within seconds.
- Parsed technical details: says the method emphasizes variable-length input handling.
- Parsed technical details: says additional observations can be used incrementally to improve reconstruction quality.
- Parsed technical details: notes pruning of redundant 3D Gaussian primitives because primitive count can grow linearly with input frame count in incremental scenarios.

## Methods / Approach

- Uses a Large Gaussian Reconstruction Transformer.
- Aggregates and registers face tokens.
- Injects 3D positional prompts.
- Predicts canonical 3D Gaussian Splatting attributes.
- Uses camera, expression, and head-pose guidance.
- Uses landmark tracking loss and sliced fusion loss for alignment and incremental fusion.
- Prunes redundant Gaussian primitives in incremental scenarios.

## Datasets, Benchmarks, Metrics

- The parsed text does not list specific datasets, benchmarks, or numeric metrics.
- It reports qualitative timing as reconstruction "within seconds" in the parsed abstract.

## Findings

- The source presents variable-length and incremental input handling as important for real-world avatar observations.
- The source highlights primitive-count growth as a practical issue for incremental Gaussian aggregation.

## Limitations

- The source is domain-specific to avatar/head reconstruction, not general scene reconstruction.
- The source uses 3D Gaussian Splatting rather than point maps as the final representation in the parsed text.
- The parsed extraction is compact and does not include full experimental tables.

## Future Work

- No explicit future-work items were present in the parsed text.

## Relevance to Current Run

- Narrow supporting relevance. FastAvatar is useful for fast and incremental video-based reconstruction patterns, but it should not anchor claims about general online scene point-map reconstruction.

## Reader Inference

- FastAvatar can support discussion of variable-length observation handling and incremental fusion in a specialized domain.
- Later synthesis should clearly mark it as avatar-specific and Gaussian-based.

## Links to Topics

- incremental-3d-reconstruction
- gaussian-splatting
- variable-length-video-input
- avatar-reconstruction

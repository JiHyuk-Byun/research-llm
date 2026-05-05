---
source_id: iclr2026-DTQIjngDta
title: "$\\pi^3$: Permutation-Equivariant Visual Geometry Learning"
source_type: paper
date: 2026-01-26
venue: "ICLR 2026 Poster"
raw_path: runs/run-1777971225/raw/sources/iclr2026-DTQIjngDta.txt
parsed_path: runs/run-1777971225/parsed/sources/iclr2026-DTQIjngDta.txt
official_url: https://openreview.net/forum?id=DTQIjngDta
pdf_url: https://openreview.net/pdf?id=DTQIjngDta
---

# $\pi^3$: Permutation-Equivariant Visual Geometry Learning

## Main Claims

- pi^3 is a feed-forward visual-geometry model that avoids reliance on a fixed reference view.
- The model predicts affine-invariant camera poses and scale-invariant local point maps without reference frames.
- The source claims state-of-the-art performance across camera pose estimation, monocular/video depth estimation, and dense point map reconstruction.

## Evidence and Locations

- Abstract: says fixed reference views can lead to instability and failure when the selected reference is suboptimal.
- Abstract: describes a fully permutation-equivariant architecture.
- Official OpenReview visible text: TL;DR says pi^3 reconstructs 3D geometry without a fixed reference view and improves robustness and accuracy for camera pose and depth estimation.

## Methods / Approach

- Uses a fully permutation-equivariant architecture.
- Predicts affine-invariant camera poses.
- Predicts scale-invariant local point maps without reference frames.

## Datasets, Benchmarks, Metrics

- The parsed source lists tasks: camera pose estimation, monocular/video depth estimation, and dense point map reconstruction.
- Specific datasets, metrics, and numbers are not available in the parsed source.

## Findings

- The source claims robustness to input ordering and higher accuracy/performance.

## Limitations

- The source is a general visual-geometry model rather than a streaming-video reconstruction method.
- The official OpenReview PDF was not preserved in this run because shell DNS resolution failed during ingest.

## Future Work

- No explicit future work is available in the parsed source text.

## Relevance to Current Run

- Medium relevance: useful as a foundation or comparator for point-map reconstruction; not itself primarily about online streaming video.

## Reader Inference

- pi^3 may be relevant because LASER explicitly names pi^3 as an offline model whose memory complexity limits streaming use. This connection relies on the LASER source plus this source's point-map model description, so later synthesis should cite both if making that claim.

## Links to Topics

- point-map-foundation-models
- permutation-equivariance
- reference-free-reconstruction
- camera-pose-estimation

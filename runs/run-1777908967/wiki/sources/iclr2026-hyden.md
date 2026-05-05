---
source_id: iclr2026-hyden
title: "Hyden: A Hybrid Dual-Path Encoder for Monocular Geometry of High-resolution Images"
source_type: paper
date: 2026
raw_path: runs/run-1777908967/raw/sources/iclr2026-hyden.openreview.txt
parsed_path: runs/run-1777908967/parsed/sources/iclr2026-hyden.txt
url: https://openreview.net/forum?id=2eL6yXLCh8
---

# Hyden: A Hybrid Dual-Path Encoder for Monocular Geometry of High-resolution Images

## Main Claims

- The source claims Hyden is a hybrid dual-path encoder for high-resolution monocular geometry estimation.
- The source claims the model estimates monocular depth, point maps, and surface normals.
- The source claims its architecture constrains transformer computation to fixed resolution while preserving high-resolution detail through a CNN branch.

## Evidence and Locations

- Parsed content: identifies Hyden as an ICLR 2026 Poster about high-resolution monocular geometry estimation.
- OpenReview abstract metadata in parsed content: describes a low-resolution Vision Transformer branch, a full-resolution CNN branch, and lightweight MLP fusion.
- Parsed technical elements: says Hyden is integrated into DepthAnything-v2 for depth and MoGe2 for surface-normal and metric point-map prediction.

## Methods / Approach

- Combines a low-resolution Vision Transformer branch for global context with a full-resolution CNN branch for fine detail.
- Fuses features with a lightweight MLP before decoding.
- Uses self-distillation with pseudo-labels from existing models at lower-resolution full-image and high-resolution crop levels.
- Applies the architecture to depth estimation and to surface-normal and metric point-map prediction.

## Datasets, Benchmarks, Metrics

- The parsed metadata does not list specific datasets, benchmarks, or numeric metrics.

## Findings

- The source presents a design pattern for efficient high-resolution geometry estimation by limiting transformer resolution and relying on CNN scaling for detail.
- The source connects efficient image geometry estimation with metric point-map prediction through MoGe2 integration.

## Limitations

- Browser PDF extraction did not return usable text during ingest, so this note is based on OpenReview forum metadata rather than full paper text.
- The source is image-focused and explicitly not presented in the parsed metadata as online streaming reconstruction.

## Future Work

- No explicit future-work items were present in the parsed metadata.

## Relevance to Current Run

- Supporting relevance. Hyden informs efficient point-map/depth/normal components, but it is not primary evidence for streaming-video reconstruction.

## Reader Inference

- Hyden may be useful for discussing efficient geometry backbones that could feed a streaming reconstruction pipeline.
- Later synthesis should keep Hyden separate from methods that directly consume and update video streams online.

## Links to Topics

- pointmap-representation
- monocular-geometry-estimation
- efficient-geometry-backbones
- high-resolution-depth-estimation

---
source_id: iclr2026-streamsplat
title: "StreamSplat: Towards Online Dynamic 3D Reconstruction from Uncalibrated Video Streams"
source_type: paper
date: 2026
raw_path: runs/run-1777908967/raw/sources/iclr2026-streamsplat.openreview.txt
parsed_path: runs/run-1777908967/parsed/sources/iclr2026-streamsplat.txt
url: https://openreview.net/forum?id=SaiDRQU7Ez
---

# StreamSplat: Towards Online Dynamic 3D Reconstruction from Uncalibrated Video Streams

## Main Claims

- The source claims StreamSplat is an online dynamic 3D reconstruction framework for arbitrary-length uncalibrated video streams.
- The source claims the framework is fully feed-forward and converts video streams into dynamic 3D Gaussian Splatting representations online.
- The source reports a 1200x speedup over optimization-based methods.

## Evidence and Locations

- Parsed content: identifies the paper as an ICLR 2026 Poster on online dynamic 3D reconstruction from uncalibrated video streams.
- OpenReview abstract metadata in parsed content: motivates the method by noting that dynamic reconstruction methods often depend on per-scene optimization and full-sequence access, creating latency and memory constraints.
- Parsed technical elements: lists probabilistic sampling, bidirectional deformation fields, adaptive Gaussian fusion, and experiments on dynamic and static benchmarks.

## Methods / Approach

- Uses probabilistic sampling to predict 3D Gaussians from uncalibrated inputs.
- Uses a bidirectional deformation field for frame associations and reduced long-term error accumulation.
- Uses adaptive Gaussian fusion to propagate persistent Gaussians while handling appearing and disappearing Gaussians.
- Avoids per-scene optimization according to the source metadata, using a feed-forward online pipeline instead.

## Datasets, Benchmarks, Metrics

- The parsed metadata says experiments cover dynamic and static benchmarks.
- The parsed metadata reports a 1200x speedup over optimization-based methods.
- Specific benchmark names, accuracy metrics, and table locations were not available in the parsed metadata.

## Findings

- The source frames online dynamic reconstruction as constrained by latency and memory when full-sequence access or per-scene optimization is required.
- The source presents adaptive fusion and deformation modeling as mechanisms for maintaining dynamic 3D Gaussian representations over streams.

## Limitations

- Browser PDF extraction did not return usable text during ingest, so this note is based on OpenReview forum metadata rather than full paper text.
- The source uses dynamic 3D Gaussian Splatting rather than point maps as its stated representation, making it adjacent to but not identical with point-map reconstruction.

## Future Work

- No explicit future-work items were present in the parsed metadata.

## Relevance to Current Run

- High relevance for online video-stream 3D reconstruction under latency and memory constraints.
- Representation relevance is indirect for point maps because the source centers on dynamic 3D Gaussian Splatting.

## Reader Inference

- StreamSplat should be used as primary evidence for online, uncalibrated, dynamic video reconstruction, but not as direct evidence for point-map output unless fuller paper text shows a point-map component.
- Because this note is metadata-only, later synthesis should treat the 1200x speedup as a reported claim from the abstract metadata and avoid unpacking it without benchmark details.

## Links to Topics

- online-streaming-3d-reconstruction
- dynamic-3d-reconstruction
- gaussian-splatting
- uncalibrated-video-streams

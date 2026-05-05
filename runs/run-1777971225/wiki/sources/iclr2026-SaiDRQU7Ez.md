---
source_id: iclr2026-SaiDRQU7Ez
title: "StreamSplat: Towards Online Dynamic 3D Reconstruction from Uncalibrated Video Streams"
source_type: paper
date: 2026-01-26
venue: "ICLR 2026 Poster"
raw_path: runs/run-1777971225/raw/sources/iclr2026-SaiDRQU7Ez.txt
parsed_path: runs/run-1777971225/parsed/sources/iclr2026-SaiDRQU7Ez.txt
official_url: https://openreview.net/forum?id=SaiDRQU7Ez
pdf_url: https://openreview.net/pdf?id=SaiDRQU7Ez
---

# StreamSplat: Towards Online Dynamic 3D Reconstruction from Uncalibrated Video Streams

## Main Claims

- StreamSplat is a feed-forward framework for online dynamic 3D reconstruction from uncalibrated video streams.
- The method transforms arbitrary-length video streams into dynamic 3D Gaussian Splatting representations online.
- The source claims state-of-the-art reconstruction quality and dynamic scene modeling, plus a 1200x speedup over optimization-based methods.

## Evidence and Locations

- Official OpenReview visible text: TL;DR says the framework is efficient, scalable, feed-forward, and online for dynamic 3D reconstruction.
- Abstract: frames the problem around sparse observations, strict latency, and memory constraints.
- Abstract: lists three technical innovations: probabilistic sampling, bidirectional deformation field, and adaptive Gaussian fusion.

## Methods / Approach

- Uses probabilistic sampling to predict 3D Gaussians from uncalibrated inputs.
- Uses a bidirectional deformation field for associations across frames and reduced long-term error accumulation.
- Uses adaptive Gaussian fusion to propagate persistent Gaussians while handling newly appearing and disappearing Gaussians.

## Datasets, Benchmarks, Metrics

- The abstract mentions standard dynamic and static benchmarks.
- Specific benchmark names and metrics are not available in the parsed source.

## Findings

- StreamSplat is reported to support online reconstruction of arbitrarily long video streams.
- It claims a 1200x speedup over optimization-based methods.

## Limitations

- This source is Gaussian Splatting based, not point-map based.
- The official OpenReview PDF was not preserved in this run because shell DNS resolution failed during ingest.

## Future Work

- The parsed source states that code and models are available at a project URL, but project pages are outside this run's evidence scope.

## Relevance to Current Run

- High relevance for online streaming dynamic reconstruction constraints; medium relevance for point-map reconstruction because the representation is 3D Gaussian Splatting.

## Reader Inference

- StreamSplat is useful as an adjacent online baseline or competing representation family for streaming dynamic reconstruction. Later synthesis should avoid treating it as point-map evidence.

## Links to Topics

- online-dynamic-3d-reconstruction
- uncalibrated-video-streams
- 3d-gaussian-splatting
- adaptive-gaussian-fusion

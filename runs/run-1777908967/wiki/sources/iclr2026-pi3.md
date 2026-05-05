---
source_id: iclr2026-pi3
title: "pi^3: Permutation-Equivariant Visual Geometry Learning"
source_type: paper
date: 2026
raw_path: runs/run-1777908967/raw/sources/iclr2026-pi3.openreview.txt
parsed_path: runs/run-1777908967/parsed/sources/iclr2026-pi3.txt
url: https://openreview.net/forum?id=DTQIjngDta
---

# pi^3: Permutation-Equivariant Visual Geometry Learning

## Main Claims

- The source claims pi^3 is a feed-forward visual geometry model that removes the need for a fixed reference view.
- The source claims the method predicts affine-invariant camera poses and scale-invariant local point maps with a fully permutation-equivariant architecture.
- The source reports improvements across camera pose estimation, monocular/video depth estimation, and dense point-map reconstruction.

## Evidence and Locations

- Parsed abstract: states that pi^3 predicts affine-invariant camera poses and scale-invariant local point maps.
- Parsed technical details: says the method accepts single images, video sequences, and unordered image sets from static or dynamic scenes.
- Parsed technical details: says each point map is predicted in the corresponding frame's own camera coordinate system.
- Parsed technical details: says the model omits order-dependent components such as frame positional embeddings and special reference-view tokens.

## Methods / Approach

- Uses a permutation-equivariant architecture where permuting the input sequence produces a correspondingly permuted output sequence.
- Uses alternating view-wise and global self-attention.
- Uses a decoder to output camera pose, point map, and confidence map.
- Avoids fixed reference-view assumptions.

## Datasets, Benchmarks, Metrics

- The parsed text mentions camera pose estimation, monocular/video depth estimation, and dense point-map reconstruction evaluations.
- The parsed text reports a Sintel camera-pose ATE reduction relative to VGGT.
- The parsed text reports 57.4 FPS inference speed.
- Full metric definitions, complete tables, and exact numeric ATE values were not present in the parsed source text.

## Findings

- The source presents local point maps as a scale-invariant visual geometry output that can be predicted for varied input forms.
- The source positions permutation equivariance as a way to handle inputs without requiring a canonical order or reference view.

## Limitations

- The parsed source describes support for video sequences, but it does not frame the method primarily as online streaming reconstruction.
- The extraction is compact and does not include full experimental tables.

## Future Work

- No explicit future-work items were present in the parsed text.

## Relevance to Current Run

- Medium to high relevance. The source is directly relevant to point-map prediction and video geometry, but it is contextual rather than primary evidence for online streaming operation.

## Reader Inference

- pi^3 can support synthesis about point-map representation and feed-forward visual geometry, especially where reference-view assumptions matter.
- Later synthesis should not describe pi^3 as an online streaming method unless additional source text verifies causal or incremental processing.

## Links to Topics

- pointmap-representation
- visual-geometry-learning
- permutation-equivariance
- video-depth-estimation

---
source_id: iclr2026-stream3r
title: "STream3R: Scalable Sequential 3D Reconstruction with Causal Transformer"
source_type: paper
date: 2026
raw_path: runs/run-1777908967/raw/sources/iclr2026-stream3r.openreview.txt
parsed_path: runs/run-1777908967/parsed/sources/iclr2026-stream3r.txt
url: https://openreview.net/forum?id=RTTYGeC2Io
---

# STream3R: Scalable Sequential 3D Reconstruction with Causal Transformer

## Main Claims

- The source claims STream3R reformulates pointmap prediction as a decoder-only Transformer problem for scalable sequential 3D reconstruction from image streams.
- The source claims the method supports online 3D perception by processing incoming frames with causal attention and cached context from prior frames.
- The source claims the model predicts dense 3D geometry for incoming frames and generalizes across static and dynamic scene benchmarks.

## Evidence and Locations

- Parsed abstract: describes the core reformulation as pointmap prediction with a decoder-only Transformer for scalable sequential reconstruction.
- Parsed technical details: frames streaming visual input as continuous processing of new observations with on-the-fly reconstruction updates.
- Parsed technical details: contrasts the method with fixed-set multi-view methods that rerun full reconstruction when a new image arrives and with memory/RNN-style methods that can drift or have limited memory.
- Parsed technical details: states that outputs include local-coordinate point maps, global-coordinate point maps, camera pose, and camera intrinsics.

## Methods / Approach

- Processes frames sequentially with causal attention over accumulated observations.
- Uses cached features from past frames as context for new frames.
- Maps a stream of RGB images to per-frame 3D attributes, including local point maps, global point maps, camera pose, and camera intrinsics.
- Uses LLM-style training and inference infrastructure according to the parsed abstract.

## Datasets, Benchmarks, Metrics

- The parsed text says the model generalizes across static and dynamic scene benchmarks.
- Specific benchmark names and numeric metrics were not present in the parsed source note beyond the general benchmark description.

## Findings

- The source positions pointmap-based representations as a feed-forward dense 3D geometry format.
- The source argues that prior pairwise or global-attention pointmap approaches have scalability issues for streaming or sequential settings.

## Limitations

- The parsed extraction is a compact browser PDF text extraction plus OpenReview metadata rather than a full paper transcription.
- Specific quantitative results, ablations, and benchmark tables were not available in the parsed text used by this reader stage.

## Future Work

- No explicit future-work items were present in the parsed text.

## Relevance to Current Run

- High relevance. This is the strongest selected match for online or streaming video/image-sequence 3D point-map reconstruction within the allowed ICLR 2026 source scope.

## Reader Inference

- Because STream3R directly targets sequential image streams and outputs local/global point maps, it should be treated as primary evidence for the run topic.
- The source appears more directly aligned with "point map reconstruction" than Gaussian-only methods, but later synthesis should avoid claiming exact latency or accuracy advantages unless those numbers are recovered from fuller paper text.

## Links to Topics

- online-streaming-3d-reconstruction
- pointmap-representation
- causal-transformer-reconstruction

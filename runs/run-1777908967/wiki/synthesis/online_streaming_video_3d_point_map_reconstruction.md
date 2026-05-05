# Online Streaming Video 3D Point-Map Reconstruction

## Scope and Evidence Base

This synthesis uses the run-local source notes selected under the plan's venue scope: CVPR 2026, ICLR 2026, and ICML 2026 only. The current selected evidence set contains six accepted ICLR 2026 papers; no CVPR 2026 or ICML 2026 source notes were available in the run wiki for synthesis.

The most direct match is STream3R, which explicitly frames sequential 3D reconstruction as pointmap prediction over image streams with causal attention and cached context, producing local/global point maps, camera pose, and camera intrinsics for incoming frames ([iclr2026-stream3r](../sources/iclr2026-stream3r.md)). StreamSplat is also a direct online video-stream reconstruction paper, but its target representation is dynamic 3D Gaussian Splatting rather than point maps ([iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)). pi^3 and Hyden provide point-map representation and efficient visual-geometry context, while CogniMap3D and FastAvatar contribute adjacent evidence on persistent mapping and incremental reconstruction ([iclr2026-pi3](../sources/iclr2026-pi3.md), [iclr2026-hyden](../sources/iclr2026-hyden.md), [iclr2026-cognimap3d](../sources/iclr2026-cognimap3d.md), [iclr2026-fastavatar](../sources/iclr2026-fastavatar.md)).

## Method Map

| Paper | Directness for query | Input setting | Output representation | Streaming / online mechanism | Main caution |
|---|---:|---|---|---|---|
| STream3R | High | Sequential image/video streams | Local point maps, global point maps, camera pose, intrinsics | Causal decoder-only Transformer with cached prior-frame context | Source note lacks full benchmark tables and exact metrics ([iclr2026-stream3r](../sources/iclr2026-stream3r.md)) |
| StreamSplat | High for online video; indirect for point maps | Arbitrary-length uncalibrated video streams | Dynamic 3D Gaussian Splatting | Feed-forward online pipeline with probabilistic sampling, bidirectional deformation, adaptive Gaussian fusion | Metadata-only note; Gaussian representation rather than point maps ([iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)) |
| pi^3 | Medium-high | Single images, video sequences, unordered image sets | Camera poses, local point maps, confidence maps | Feed-forward permutation-equivariant geometry model, not described as causal streaming | Should not be called online without more evidence ([iclr2026-pi3](../sources/iclr2026-pi3.md)) |
| CogniMap3D | Medium | Image streams and revisits | Persistent 3D mapping, pose/depth/memory outputs | Static-scene memory bank, retrieval, camera relocation, factor-graph refinement | Metadata-only note; no point-map output identified ([iclr2026-cognimap3d](../sources/iclr2026-cognimap3d.md)) |
| Hyden | Supporting | High-resolution monocular images | Depth, point maps, surface normals | Efficient image backbone; not streaming | Image-focused, not online sequence reconstruction ([iclr2026-hyden](../sources/iclr2026-hyden.md)) |
| FastAvatar | Narrow supporting | Single image, multi-view, monocular video | 3D Gaussian avatar model | Variable-length/incremental reconstruction with fusion and primitive pruning | Avatar-specific and Gaussian-based ([iclr2026-fastavatar](../sources/iclr2026-fastavatar.md)) |

## Agreements and Trends

The central 2026 trend in this evidence set is movement away from per-scene optimization or fixed full-sequence reconstruction toward feed-forward or cached sequential inference. STream3R argues that fixed-set multi-view methods are poorly matched to new frames because reconstruction must be rerun, and it instead uses causal attention plus cached prior context for sequential updates ([iclr2026-stream3r](../sources/iclr2026-stream3r.md)). StreamSplat similarly frames full-sequence access and per-scene optimization as sources of latency and memory pressure, then proposes a fully feed-forward online dynamic reconstruction pipeline ([iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)).

Point maps appear as a strong intermediate or direct geometry representation, but not always as the final map. STream3R is the clearest point-map streaming system because it predicts local and global point maps for incoming frames ([iclr2026-stream3r](../sources/iclr2026-stream3r.md)). pi^3 strengthens the representation-side evidence by predicting scale-invariant local point maps and affine-invariant camera poses across images, videos, and unordered image sets ([iclr2026-pi3](../sources/iclr2026-pi3.md)). Hyden extends the point-map thread to high-resolution monocular geometry estimation by integrating with MoGe2 for metric point-map prediction ([iclr2026-hyden](../sources/iclr2026-hyden.md)).

For longer-running or dynamic scenes, the evidence shifts from single-shot geometry prediction toward persistence, association, and fusion. StreamSplat uses bidirectional deformation fields and adaptive Gaussian fusion to maintain dynamic Gaussian representations over video streams ([iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)). CogniMap3D uses a persistent memory bank for static scenes, dynamic-region handling through motion cues, scene retrieval on revisits, and camera relocalization ([iclr2026-cognimap3d](../sources/iclr2026-cognimap3d.md)). FastAvatar, in a narrower avatar setting, highlights primitive-count growth during incremental Gaussian aggregation and uses pruning to manage it ([iclr2026-fastavatar](../sources/iclr2026-fastavatar.md)).

## Bottlenecks

The first bottleneck is sequence scaling. STream3R identifies global or fixed-set reconstruction as inefficient when new frames arrive and addresses this with causal attention and cached features ([iclr2026-stream3r](../sources/iclr2026-stream3r.md)). StreamSplat similarly targets latency and memory limits created by optimization-based dynamic reconstruction pipelines ([iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)).

The second bottleneck is stable long-horizon map maintenance. StreamSplat's adaptive Gaussian fusion and deformation modeling are aimed at appearing, disappearing, and associating geometry over time ([iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)). CogniMap3D treats revisits, static-scene retrieval, dynamic-region filtering, and memory updates as core requirements for persistent 3D mapping ([iclr2026-cognimap3d](../sources/iclr2026-cognimap3d.md)).

The third bottleneck is efficient high-resolution geometry. Hyden's architecture constrains transformer computation to low resolution while preserving high-resolution detail with a CNN branch, suggesting a component-level route for reducing dense-geometry cost ([iclr2026-hyden](../sources/iclr2026-hyden.md)). This is relevant as supporting evidence only, because Hyden is not presented in the source note as an online streaming method.

## Gaps and Unresolved Questions

Quantitative comparison is weak in the available source notes. STream3R, CogniMap3D, Hyden, and FastAvatar notes do not include full benchmark tables or detailed numeric metrics; StreamSplat's note reports a 1200x speedup but lacks the benchmark details needed to interpret that number; pi^3's note reports 57.4 FPS and a Sintel ATE reduction but not complete metric context ([iclr2026-stream3r](../sources/iclr2026-stream3r.md), [iclr2026-streamsplat](../sources/iclr2026-streamsplat.md), [iclr2026-pi3](../sources/iclr2026-pi3.md), [iclr2026-cognimap3d](../sources/iclr2026-cognimap3d.md), [iclr2026-hyden](../sources/iclr2026-hyden.md), [iclr2026-fastavatar](../sources/iclr2026-fastavatar.md)).

There is also a representation gap. The strongest online-video evidence splits between point maps and Gaussians: STream3R directly supports online point-map reconstruction, while StreamSplat directly supports online dynamic video reconstruction but with 3D Gaussian Splatting ([iclr2026-stream3r](../sources/iclr2026-stream3r.md), [iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)). A direct comparison between online point-map maps and online Gaussian maps is not available in the current notes.

Venue coverage remains incomplete. The plan allowed CVPR 2026, ICLR 2026, and ICML 2026, but the selected source notes available to synthesis are all from ICLR 2026. This should be treated as a run coverage limitation, not as evidence that no relevant CVPR 2026 or ICML 2026 work exists.

## Synthesis Interpretation

Within the scoped evidence, "online streaming video 3D point-map reconstruction" is best represented by STream3R. The surrounding ICLR 2026 papers suggest a broader research cluster: causal/cached sequential transformers for dense geometry, feed-forward online dynamic Gaussian reconstruction, permutation-equivariant point-map prediction, persistent memory for mapping, and efficient monocular geometry backbones. The strongest open research question is how to combine STream3R-like causal point-map prediction with StreamSplat/CogniMap3D-style long-horizon fusion, dynamic-scene handling, and persistent memory while retaining measurable latency and memory advantages.

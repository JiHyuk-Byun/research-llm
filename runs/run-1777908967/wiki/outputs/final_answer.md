# Final Answer

Within the scoped run evidence, the closest match to "online streaming video 3d point map reconstruction" is **STream3R: Scalable Sequential 3D Reconstruction with Causal Transformer**. It is an accepted ICLR 2026 paper that reformulates pointmap prediction as sequential decoder-only Transformer inference, uses causal attention and cached prior-frame context, and outputs local/global point maps, camera pose, and intrinsics for incoming frames ([iclr2026-stream3r](../sources/iclr2026-stream3r.md)).

The strongest adjacent online-video reconstruction paper is **StreamSplat: Towards Online Dynamic 3D Reconstruction from Uncalibrated Video Streams**. It directly targets online dynamic reconstruction from arbitrary-length uncalibrated video streams and reports a feed-forward dynamic 3D Gaussian Splatting pipeline, but it is not point-map-centered in the available note ([iclr2026-streamsplat](../sources/iclr2026-streamsplat.md)).

Other relevant ICLR 2026 papers are supporting rather than primary matches:

| Paper | Why it matters | Limitation for this query |
|---|---|---|
| **pi^3: Permutation-Equivariant Visual Geometry Learning** | Predicts camera poses and local point maps for images, videos, and unordered image sets ([iclr2026-pi3](../sources/iclr2026-pi3.md)) | Not presented as online or causal streaming |
| **CogniMap3D** | Handles image-stream mapping with persistent memory, retrieval, and camera relocalization ([iclr2026-cognimap3d](../sources/iclr2026-cognimap3d.md)) | No point-map output identified in the note |
| **Hyden** | Supports efficient high-resolution monocular depth, normals, and metric point-map estimation ([iclr2026-hyden](../sources/iclr2026-hyden.md)) | Image-focused, not streaming-video reconstruction |
| **FastAvatar** | Shows fast/incremental video-based Gaussian reconstruction for avatars ([iclr2026-fastavatar](../sources/iclr2026-fastavatar.md)) | Avatar-specific and Gaussian-based |

No selected CVPR 2026 or ICML 2026 source notes were available in the run wiki for synthesis. Therefore, the scoped answer is: **STream3R is the primary in-scope paper for online/sequential 3D point-map reconstruction; StreamSplat is the primary adjacent online video reconstruction paper, but with Gaussian Splatting rather than point maps.**

---
source_id: iclr2026-RTTYGeC2Io
title: "STream3R: Scalable Sequential 3D Reconstruction with Causal Transformer"
source_type: paper
date: 2026-01-26
venue: "ICLR 2026 Poster"
raw_path: runs/run-1777971225/raw/sources/iclr2026-RTTYGeC2Io.txt
parsed_path: runs/run-1777971225/parsed/sources/iclr2026-RTTYGeC2Io.txt
official_url: https://openreview.net/forum?id=RTTYGeC2Io
pdf_url: https://openreview.net/pdf?id=RTTYGeC2Io
---

# STream3R: Scalable Sequential 3D Reconstruction with Causal Transformer

## Main Claims

- STream3R reformulates pointmap prediction as a decoder-only Transformer problem.
- The method processes image sequences efficiently with causal attention for streaming reconstruction.
- The source claims consistent outperformance over prior work across static and dynamic scene benchmarks.

## Evidence and Locations

- Official OpenReview visible text: TL;DR states feed-forward 4D reconstruction from causal videos.
- Abstract: says existing multi-view reconstruction depends on expensive global optimization or memory mechanisms that scale poorly with sequence length.
- Abstract: states that STream3R uses a streaming framework with causal attention inspired by modern language modeling.

## Methods / Approach

- Uses a decoder-only Transformer formulation for pointmap prediction.
- Uses causal attention to process image sequences.
- Learns geometric priors from large-scale 3D datasets.
- Is described as compatible with LLM-style training infrastructure.

## Datasets, Benchmarks, Metrics

- The abstract mentions static and dynamic scene benchmarks.
- Specific benchmark names, metrics, and quantitative results are not available in the parsed source.

## Findings

- The source claims scalability to diverse and challenging scenarios, including dynamic scenes.
- It presents causal Transformers as promising for online 3D perception and real-time streaming environments.

## Limitations

- The official OpenReview PDF was not preserved in this run because shell DNS resolution failed during ingest.
- Parsed evidence is limited to OpenReview visible text and abstract.

## Future Work

- The abstract points to downstream pretraining and fine-tuning for various 3D tasks as enabled by the approach, but does not list concrete future work items.

## Relevance to Current Run

- Very high relevance: directly links pointmap prediction, sequential/streaming processing, causal attention, online 3D perception, and real-time streaming environments.

## Reader Inference

- STream3R likely provides the clearest causal-transformer architecture source for streaming pointmap reconstruction in this run. This is inference from the abstract's formulation and the selected-source rationale.

## Links to Topics

- streaming-pointmap-prediction
- causal-transformers
- sequential-3d-reconstruction
- online-3d-perception

# Third-party notices

- Korean helper integration: kinam-ssafy/torchlight-overlay, MIT, based on ddac4bfce7fe7551351d5cac7d8ef35096b2100d. Original notices and licences are retained.

- TartarusSolver, glcoder, MIT. Based on v0.7.20 (9e19dda). https://github.com/glcoder/TartarusSolver
- Vorax Solver Rust, DinoRaos and original contributors including tidewarden and Moebing, MIT. Based on 9aabc20. https://github.com/DinoRaos/vorax-solver-rs
- The original MIT notices are included under `licenses/` and in each source tree. No source from tidewarden after MIT commit 7b4c9ba is incorporated.
- .NET and WPF: Microsoft, MIT. https://github.com/dotnet/runtime and https://github.com/dotnet/wpf
- ONNX Runtime: Microsoft, MIT. https://github.com/microsoft/onnxruntime
- PaddleOCR models: PaddlePaddle authors, Apache-2.0. https://github.com/PaddlePaddle/PaddleOCR
- OCR conversions: https://huggingface.co/monkt/paddleocr-onnx and https://huggingface.co/SWHL/RapidOCR
- Sentry .NET SDK: Functional Software, MIT. https://github.com/getsentry/sentry-dotnet (network reporting disabled in this build).
- ClrMD: Microsoft, MIT. https://github.com/microsoft/clrmd
- Additional Rust and NuGet dependency license files are collected under `licenses/dependencies/`. Cargo.lock and the C# project files record package versions.

Torchlight: Infinite names, text, pictures and recognition templates belong to their respective rights holders. This is an unofficial fan utility, not endorsed by XD. Korean game terminology was checked against https://tlidb.com/ko/Vorax_Season and game facts listed at https://github.com/madstone290/VoraxRoom (no code from the latter is included).

Modifications in this distribution: Korean UI and OCR aliases; portable configuration; content engine switching; local-only diagnostics; uncertain-reading suppression; configurable futures and bounded CPU parallelism. Existing game engines and trained values are preserved.

Additional modifications in 1.1.0: bounded numeric-region recovery with product validation; translated-card art fallback; one supplemental Korean round-number glyph; zero-reroll instrument-screen handling; maximum CPU/OCR mode; production-path diagnostics and detailed unresolved-input logs. No new runtime package dependency is introduced.

Additional modifications in 1.1.1: log-independent visual change detection, stale-advice invalidation, isolated Korean title OCR and validated round-counter OCR fallback, and supplemental samples from a reported round-6 frame. No runtime dependency added.

Additional modifications in 1.2.0: user-assisted enlarged-card OCR, blue unresolved-slot markers, persistent capture sessions, shape-based UI change detection, and bounded local diagnostic captures. No runtime package dependency added.

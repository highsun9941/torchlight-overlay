# Korean CPU OCR experiment

Branch: `hf-korean-ocr-cpu`; baseline: v1.2.7. This branch is separate from main.

## Model selection

The baseline Vorax reader uses Windows.Media.Ocr. Its recorded game-font errors motivate testing an independent recognizer. Tartarus already ships the Korean PP-OCRv5 mobile ONNX conversion, but Vorax did not call it.

| Candidate | Evidence | Decision |
| --- | --- | --- |
| Korean PP-OCRv5 mobile | Explicit Korean/English/digit support; Apache-2.0; bundled ONNX is 13,401,252 bytes | First candidate: reuse packaged weights and ONNX Runtime |
| Korean PP-OCRv3 mobile | Officially smaller/faster, but lower Korean accuracy on the publisher benchmark | Keep as a possible speed comparison; not shipped in this experiment |
| PP-OCRv6 tiny | Official ONNX around 4.46 MB; general 49-language recognizer | Needs Korean alphabet and game-font validation before adoption; no claim that its aggregate score is comparable to the Korean benchmark |
| OCR visual language models | Much larger runtimes and weights than the already bundled mobile recognizer | Not selected for this CPU-focused build |

Primary sources:

- https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec
- https://huggingface.co/monkt/paddleocr-onnx/blob/main/languages/korean/config.json
- https://www.paddleocr.ai/main/en/version3.x/module_usage/text_recognition.html
- https://huggingface.co/PaddlePaddle/PP-OCRv6_tiny_rec_onnx

The publisher's general accuracy and latency are not measurements of this game or the user's PC. The ONNX conversion's preprocessing metadata says height 32; the actual model input metadata is inspected at runtime. Existing model and dictionary SHA-256 pins are verified before inference.

## Runtime

The native overlay keeps a single console OCR worker, communicates through bounded local binary pipes, and sends cropped RGBA text inputs. The worker loads only the recognition network, segments rows in the existing dark-on-white crops, and uses CPU inference with one inter-op thread, two intra-op threads, sequential execution and idle spinning disabled. No Python runtime or model download is needed on the user's machine.

`VORAX_OCR_BACKEND=paddle` is the Korean branch default. `windows` explicitly selects the former engine; non-Korean languages retain Windows OCR. `VORAX_OCR_THREADS=1` allows a lower CPU setting. A worker startup/read failure is reported; the experiment does not silently swap to the former engine. EOF ends the worker, and the parent kills/reaps it on normal shutdown or timeout.

## Verification

Actual packaged model inference is required on the saved Bone Saw, Adherent Metatarsals, Green Bile and normal Twin Hormone tooltips, followed by the Bone Saw pipeline into advice. No supplied OCR strings are used in this validation. Inputs, raw model outputs, confidence, timings and memory are saved as a CI artifact. One- and two-thread benchmarks bypass the overlay text cache and repeat each input six times; the first run is cold and subsequent runs are warm.

Actual outputs and measurements are stored in the `Korean-CPU-OCR-validation` CI artifact associated with the tested commit; see `model-validation.json` and `benchmark-1.json` / `benchmark-2.json`. Windows OCR text in earlier user logs is an accuracy comparison on saved captures, not a simultaneous timing benchmark on the same hardware.

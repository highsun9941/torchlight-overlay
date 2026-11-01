# Korean CPU OCR experiment

Branch: `hf-korean-ocr-cpu`; baseline: v1.2.7. This branch is separate from main.

## Model selection

The baseline Vorax reader uses Windows.Media.Ocr. Its recorded game-font errors motivate testing an independent recognizer. Tartarus already ships the Korean PP-OCRv5 mobile ONNX conversion, but Vorax did not call it.

| Candidate | Evidence | Decision |
| --- | --- | --- |
| Korean PP-OCRv5 mobile | Explicit Korean/English/digit support; Apache-2.0; bundled ONNX is 13,401,252 bytes | First candidate: reuse packaged weights and ONNX Runtime |
| Korean PP-OCRv3 mobile | Officially smaller/faster, but lower Korean accuracy on the publisher benchmark | Keep as a possible speed comparison; not shipped in this experiment |
| PP-OCRv6 tiny | Official ONNX around 4.46 MB; general 49-language recognizer | Excluded: the published CTC dictionary lacks the target Hangul syllables 무/쇠/뼈/톱; a smaller file cannot recognize the required title |
| OCR visual language models | Much larger runtimes and weights than the already bundled mobile recognizer | Not selected for this CPU-focused build |

Primary sources:

- https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec
- https://huggingface.co/monkt/paddleocr-onnx/blob/main/languages/korean/config.json
- https://www.paddleocr.ai/main/en/version3.x/module_usage/text_recognition.html
- https://huggingface.co/PaddlePaddle/PP-OCRv6_tiny_rec_onnx

The publisher's general accuracy and latency are not measurements of this game or the user's PC. The ONNX conversion's preprocessing metadata says height 32; the actual model input metadata is inspected at runtime. Existing model and dictionary SHA-256 pins are verified before inference.

## Runtime

The native overlay keeps a single console OCR worker, communicates through bounded local binary pipes, and sends cropped RGBA text inputs. The worker loads only the recognition network, segments rows in the existing dark-on-white crops, and uses CPU inference with one inter-op thread, one intra-op thread, sequential execution and idle spinning disabled. No Python runtime or model download is needed on the user's machine.

`VORAX_OCR_BACKEND=paddle` is the Korean branch default. `windows` explicitly selects the former engine; non-Korean languages retain Windows OCR. `VORAX_OCR_THREADS=2` selects the faster two-thread option. A worker startup/read failure is reported; the experiment does not silently swap to the former engine. EOF ends the worker, and the parent kills/reaps it on normal shutdown or timeout.

## Verification

Actual packaged model inference is required on the saved Bone Saw, Adherent Metatarsals, Green Bile and normal Twin Hormone tooltips, followed by the Bone Saw pipeline into advice. No supplied OCR strings are used in this validation. Inputs, raw model outputs, confidence, timings and memory are saved as a CI artifact. One- and two-thread benchmarks bypass the overlay text cache and repeat each input six times; the first run is cold and subsequent runs are warm.

Actual outputs and measurements are stored in the `Korean-CPU-OCR-validation` CI artifact associated with the tested commit; see `model-validation.json` and `benchmark-1.json` / `benchmark-2.json`. Windows OCR text in earlier user logs is an accuracy comparison on saved captures, not a simultaneous timing benchmark on the same hardware.

## First actual-model run

CI run 41309555547 (source c4836a1393567e4bbb78c937a7654b9d7495edaa) loaded the real model and read the saved inputs. All four tooltip routes and the Saw pipeline passed. The Saw remained unresolved on the small card, then its enlarged heading was read exactly as `무쇠뼈톱` and reached advice. Adherent Metatarsals produced `엉켜분은 척골`, Green Bile `푸른 담즙용액`, and Twin Hormone was resolved using its heading plus explicit Pest evidence in its description. Thus four catalog matches passed; this does not mean every raw syllable was exact.

Across six cropped inputs, warm inference medians (excluding segmentation and cold startup) were 25.55895 ms with one CPU thread and 22.82495 ms with two. Worker working sets were 105.20 and 108.36 MiB respectively. The one-thread default trades about 2.7 ms for a lower CPU concurrency budget. These are CI measurements, not gameplay FPS or guarantees for the user's machine. The actual reader used height 48, despite the conversion repository's metadata example saying 32. The Saw heading's initial model confidence was 0.9974; confidence is a model score, not a calibrated accuracy claim.

That run did not finish the helper UI check after the console worker was published into the same output folder. Worker publication now uses staging and copies only its four application files; the helper's Windows Desktop runtime files remain authoritative. A preview error now exits with a diagnostic instead of waiting on a modal dialog. The next full check must verify the final package.

V6 alphabet inspected: https://huggingface.co/PaddlePaddle/PP-OCRv6_tiny_rec_onnx/blob/main/inference.yml

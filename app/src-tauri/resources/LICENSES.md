# Bundled resources

## speed-sample.wav

A 10 second excerpt (starting at 1:00) of the LibriVox recording of "Pride and Prejudice" by
Jane Austen (version 2), read by Annie Coleman, converted to 16 kHz mono 16-bit WAV.

- Source: https://archive.org/details/solo_pride_librivox
  (file `prideandprejudice_01-04_austen_apc_64kb.mp3`)
- LibriVox: https://librivox.org/
- Licence: public domain. LibriVox recordings are dedicated to the public domain
  (https://librivox.org/pages/public-domain/); the book was published in 1813.

## whisper.cpp and llama.cpp programs

The bundled `whisper-server`, `whisper-cli`, `whisper-vad-speech-segments` and `llama-server`
are built from the tags in `scripts/sidecars.env` of whisper.cpp
(https://github.com/ggml-org/whisper.cpp) and llama.cpp (https://github.com/ggml-org/llama.cpp).
Both are MIT licensed, Copyright (c) the ggml authors. The licence texts below are copied from
the pinned checkouts (whisper.cpp v1.9.5, llama.cpp b11559).

### whisper.cpp

MIT License

Copyright (c) 2023-2026 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

### llama.cpp

MIT License

Copyright (c) 2023-2026 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Downloaded models

Not bundled; downloaded on request from the pinned Hugging Face files in `models.json`.

- Qwen3 and Qwen3.5 notes models: Apache-2.0 (Qwen's published licence).
- Whisper speech models (ggml conversions of OpenAI Whisper): MIT.
- Silero VAD: MIT.

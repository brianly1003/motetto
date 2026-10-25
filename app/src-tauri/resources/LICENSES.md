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
Both are MIT licensed, Copyright (c) the ggml authors.

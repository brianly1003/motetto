#!/usr/bin/env bash
# Build whisper.cpp and llama.cpp at the pinned tags and copy the four programs the desktop
# bundles to app/src-tauri/binaries/{name}-{target-triple} (the names Tauri's externalBin wants).
# Usage: scripts/build-sidecars.sh [target-triple]   (default: the host triple from rustc)
# Needs git, cmake and a C++ toolchain. Windows runs it from Git Bash with the MSVC toolchain
# and the Vulkan SDK; that path has not been run yet.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=sidecars.env
. "$root/scripts/sidecars.env"
triple=${1:-$(rustc -vV | sed -n 's/^host: //p')}
cache=$root/target/sidecars
out=$root/app/src-tauri/binaries
mkdir -p "$cache" "$out"

exe=
case $triple in *windows*) exe=.exe ;; esac

# fetch <name> <url> <tag> <commit>: a tag can be moved, so the checkout must be the pinned commit,
# fresh or cached. A wrong cached checkout is an error, not silently replaced: look at it, then
# delete it to clone again.
fetch() {
  local dir=$cache/$1-$3
  if [ ! -d "$dir/.git" ]; then
    git clone --quiet --depth 1 --branch "$3" "$2" "$dir" >&2
  fi
  local head
  head=$(git -C "$dir" rev-parse HEAD)
  if [ "$head" != "$4" ]; then
    echo "$dir is at $head, expected $4 ($3); check sidecars.env, or delete the folder to clone again" >&2
    exit 1
  fi
  echo "$dir"
}

# build <source dir> <build dir> <cmake flags...> -- <targets...>
build() {
  local src=$1 dir=$2
  shift 2
  local flags=()
  while [ "$1" != -- ]; do flags+=("$1"); shift; done
  shift
  cmake -S "$src" -B "$dir" -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
    "${flags[@]}"
  cmake --build "$dir" --config Release -j --target "$@"
}

# install <build dir> <program> <suffix>
install_program() {
  local file
  file=$(find "$1/bin" -name "$2$exe" -type f | head -n 1)
  [ -n "$file" ] || { echo "missing $2 in $1" >&2; exit 1; }
  cp "$file" "$out/$2$3-$triple$exe"
}

whisper=$(fetch whisper.cpp https://github.com/ggml-org/whisper.cpp "$WHISPER_CPP_TAG" "$WHISPER_CPP_COMMIT")
llama=$(fetch llama.cpp https://github.com/ggml-org/llama.cpp "$LLAMA_CPP_TAG" "$LLAMA_CPP_COMMIT")
whisper_programs=(whisper-server whisper-cli whisper-vad-speech-segments)
# llama-server only serves plain HTTP on localhost and the app downloads models itself: no TLS.

case $triple in
  *apple-darwin)
    # The Metal library is embedded so the programs run from any folder.
    gpu=(-DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON)
    build "$whisper" "$cache/build-whisper" "${gpu[@]}" -- "${whisper_programs[@]}"
    build "$llama" "$cache/build-llama" "${gpu[@]}" -DLLAMA_OPENSSL=OFF -- llama-server
    for p in "${whisper_programs[@]}"; do install_program "$cache/build-whisper" "$p" ""; done
    install_program "$cache/build-llama" llama-server ""
    ;;
  *windows*)
    # A Vulkan build and a CPU build; the app picks one at run time.
    for variant in vulkan cpu; do
      if [ $variant = vulkan ]; then gpu=(-DGGML_VULKAN=ON); else gpu=(-DGGML_VULKAN=OFF); fi
      build "$whisper" "$cache/build-whisper-$variant" "${gpu[@]}" -- "${whisper_programs[@]}"
      build "$llama" "$cache/build-llama-$variant" "${gpu[@]}" -DLLAMA_OPENSSL=OFF -- llama-server
      for p in "${whisper_programs[@]}"; do
        install_program "$cache/build-whisper-$variant" "$p" "-$variant"
      done
      install_program "$cache/build-llama-$variant" llama-server "-$variant"
    done
    ;;
  *)
    echo "unsupported target $triple" >&2
    exit 1
    ;;
esac

ls -l "$out"

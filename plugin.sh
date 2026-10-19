#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/gray-goal.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/gray-goal" ;;
    *) rel="x86_64-apple-darwin/gray-goal" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/gray-goal" ;;
    *) rel="x86_64-unknown-linux-musl/gray-goal" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/gray-goal.exe" "$here/target/release/gray-goal" "$here/target/debug/gray-goal.exe" "$here/target/debug/gray-goal"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "gray-goal binary not found in $here" >&2
exit 1

#!/bin/sh
# Prints the formula for tag $1 (e.g. v0.1.0) with url and sha256 filled in.
# Pipe it into Formula/resh.rb of the homebrew-resh tap, then commit and push there.
set -eu
tag="$1"
url="https://github.com/wongjiahau/resh/archive/refs/tags/$tag.tar.gz"
sha=$(curl -fsSL "$url" | shasum -a 256 | cut -d' ' -f1)
dir=$(dirname "$0")
sed -e "s|^  url .*|  url \"$url\"|" -e "s|^  sha256 .*|  sha256 \"$sha\"|" "$dir/resh.rb"

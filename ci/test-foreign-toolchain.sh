#!/usr/bin/env bash
# shellcheck disable=SC1091,SC1090

# test to see that foreign toolchains work

set -x
set -eo pipefail

ci_dir=$(dirname "${BASH_SOURCE[0]}")
ci_dir=$(realpath "${ci_dir}")
. "${ci_dir}"/shared.sh
project_home=$(dirname "${ci_dir}")

main() {
    local td=

    retry cargo fetch
    cargo build
    export CROSS="${project_home}/target/debug/cross"

    td="$(mkcargotemp -d)"

    pushd "${td}"
    cargo init --bin --name foreign_toolchain
    # shellcheck disable=SC2016
    echo '# Cross.toml
[build]
default-target = "x86_64-unknown-linux-musl"

[target."x86_64-unknown-linux-musl"]
image.name = "alpine:edge"
image.toolchain = ["x86_64-unknown-linux-musl"]
pre-build = ["apk add --no-cache gcc musl-dev"]' > Cross.toml

    "$CROSS" run -v

    echo '# Cross.toml
[build]
default-target = "x86_64-unknown-linux-gnu"

[target.x86_64-unknown-linux-gnu]
pre-build = [
    "apt-get update && apt-get install -y libc6 g++-x86-64-linux-gnu libc6-dev-amd64-cross",
]

[target.x86_64-unknown-linux-gnu.env]
passthrough = [
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc",
    "CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc",
    "CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++",
]

[target.x86_64-unknown-linux-gnu.image]
name = "ubuntu:20.04"
toolchain = ["aarch64-unknown-linux-gnu"]
    ' > Cross.toml

    "$CROSS" run -v

    popd

    rm -rf "${td}"
}

main

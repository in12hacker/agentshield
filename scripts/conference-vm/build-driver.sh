#!/bin/bash
# Build-only dependencies for the conference guest; enforcement stays in build.sh.
set -euo pipefail
[[ $# == 1 && $1 == /* && ! -e $1 && $(systemd-detect-virt) == qemu ]]
src=/mnt/payload
build=$1
prefix=/opt/as-conference/build-tools/libbpf
printf '%s  %s\n' 989ed3c1a3db8ff0f7c08dd43953c6b9d0c3ac252653a48d566aaedf98bc80ca "$src/libbpf-1.6.3.tar.gz" | sha256sum --check
mkdir -p "$build" "$prefix" /opt/as-conference/build-tools
tar -xf "$src/libbpf-1.6.3.tar.gz" -C "$build"
make -j8 -C "$build/libbpf-1.6.3/src" PREFIX="$prefix" LIBDIR="$prefix/lib" install
cat >/opt/as-conference/build-tools/pahole <<'PAHOLE'
#!/bin/sh
LD_LIBRARY_PATH=/opt/as-conference/build-tools/libbpf/lib exec /usr/bin/pahole "$@"
PAHOLE
chmod 0755 /opt/as-conference/build-tools/pahole
mkdir -p "$build/driver-source/drivers" "$build/driver-source/crates/backend/as-driver-adapter"
cp -a "$src/drivers/linux" "$build/driver-source/drivers/linux"
cp -a "$src/crates/backend/as-driver-adapter/native" "$build/driver-source/crates/backend/as-driver-adapter/native/"
MAKEFLAGS=-j8 CLANG=clang PAHOLE=/opt/as-conference/build-tools/pahole \
    bash "$build/driver-source/drivers/linux/build.sh" all "$build/native"
printf 'AS_CONFERENCE_NATIVE_BUILT %s\n' "$build/native"

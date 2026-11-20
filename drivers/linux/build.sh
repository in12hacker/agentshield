#!/usr/bin/env bash
# Build against the selected kernel's Kbuild and BTF.
set -euo pipefail
if [[ $# != 2 || ( "$1" != module && "$1" != all ) ]]; then
    echo 'usage: build.sh module|all OUTPUT_DIRECTORY' >&2
    exit 2
fi
mode=$1
out=$(realpath -m -- "$2")
src=$(cd -- "$(dirname -- "$0")" && pwd)
adapter_native="$src/../../crates/backend/as-driver-adapter/native"
running_release=$(uname -r)
release=${KERNEL_RELEASE:-$running_release}
kernel_build=${KERNEL_BUILD:-/lib/modules/$release/build}
header_release=$(sed -n 's/^#define UTS_RELEASE "\([^"]*\)"$/\1/p' "$kernel_build/include/generated/utsrelease.h")
if [[ "$header_release" != "$release" ]]; then
    echo "headers report $header_release, expected kernel $release" >&2
    exit 1
fi
case $(uname -m) in
    aarch64) bpf_arch=arm64; kernel_arch=ARM64 ;;
    x86_64) bpf_arch=x86; kernel_arch=X86_64 ;;
    *) echo 'supported build architectures: aarch64, x86_64' >&2; exit 1 ;;
esac
if ! grep -Fxq "CONFIG_${kernel_arch}=y" "$kernel_build/include/config/auto.conf"; then
    echo "target $release is not configured for the local build architecture" >&2
    exit 1
fi
live_btf=false
btf_base="$kernel_build/vmlinux"
if [[ -e "$btf_base" ]]; then
    if ! readelf --wide --sections "$btf_base" | grep -E '[[:space:]]\.BTF[[:space:]]' >/dev/null; then
        echo "target $release requires a vmlinux ELF containing BTF" >&2
        exit 1
    fi
elif [[ "$release" == "$running_release" && -r /sys/kernel/btf/vmlinux && -r "$kernel_build/scripts/gen-btf.sh" ]]; then
    btf_base=/sys/kernel/btf/vmlinux
    live_btf=true
else
    echo "target $release requires its own matching vmlinux with BTF in $kernel_build" >&2
    exit 1
fi
mkdir -p -- "$out/module"
ko=$out/module/as_wait.ko
# Live-base BTF resolution rewrites and sorts the linked .BTF_ids in place.
# Re-link from Kbuild objects before resolving again, including a retry after
# a later BPF/loader build failure.
if "$live_btf"; then rm -f -- "$ko"; fi
make -C "$kernel_build" M="$src" MO="$out/module" PAHOLE="${PAHOLE:-pahole}" modules
if "$live_btf"; then
    objcopy --remove-section .BTF --remove-section .BTF.base "$ko"
    objtree="$kernel_build" PAHOLE="${PAHOLE:-pahole}" PAHOLE_FLAGS="${PAHOLE_FLAGS:-}" \
    RESOLVE_BTFIDS="$kernel_build/tools/bpf/resolve_btfids/resolve_btfids" \
    RESOLVE_BTFIDS_FLAGS=--distill_base OBJCOPY=objcopy \
    sh "$kernel_build/scripts/gen-btf.sh" --btf_base "$btf_base" "$ko"
fi
sections=$(readelf --wide --sections "$ko")
if [[ ! "$sections" =~ [[:space:]]\.BTF[[:space:]] || ! "$sections" =~ [[:space:]]\.BTF_ids[[:space:]] ]]; then
    echo "module for $release is missing BTF or resolved BTF ID data" >&2
    exit 1
fi
if [[ "$mode" == module ]]; then exit 0; fi
bpftool btf dump file "$btf_base" format c > "$out/vmlinux.h"
"${CLANG:-clang}" -g -O2 -target bpf -D"__TARGET_ARCH_$bpf_arch" \
    -isystem "$out" -I "${LIBBPF_INCLUDE:-/usr/include}" -Wall -Werror \
    -c "$src/as_hooks.bpf.c" -o "$out/as_hooks.bpf.o"
# A cold-recovery loader must not read an unbound dynamic dependency outside the protected bundle.
libbpf_static_lib=${LIBBPF_STATIC_LIB:--l:libbpf.a}
"${CC:-cc}" -static -O2 -g -Wall -Wextra -Werror -I "${LIBBPF_INCLUDE:-/usr/include}" -I "$src" \
    "$adapter_native/as_loader.c" "$libbpf_static_lib" -lelf -lz -lzstd -o "$out/as-bpf-loader"
# Controlled TLS egress uses cgroup marking and packet-only nft redirect. It leaves
# socket peer addresses intact for the existing Network Connect/Send matcher.
"${CLANG:-clang}" -g -O2 -target bpf -D"__TARGET_ARCH_$bpf_arch" \
    -isystem "$out" -I "${LIBBPF_INCLUDE:-/usr/include}" -I "$src" -Wall -Werror \
    -c "$src/as_flow.bpf.c" -o "$out/as_flow.bpf.o"
"${CC:-cc}" -static -O2 -g -Wall -Wextra -Werror -I "${LIBBPF_INCLUDE:-/usr/include}" -I "$src" \
    "$adapter_native/as_flow_loader.c" "$libbpf_static_lib" -lelf -lz -lzstd -o "$out/as-tls-ingress"

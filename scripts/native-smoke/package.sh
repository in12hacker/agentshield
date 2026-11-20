#!/usr/bin/env bash
# Build a fresh disposable guest; never load drivers on the host.
set -euo pipefail
umask 022
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
if [[ $# -lt 2 || $# -gt 4 ]]; then
    echo 'usage: package.sh NEW_OUTPUT_DIRECTORY CURRENT_NATIVE_BUILD [STATIC_BUSYBOX [CARGO_TARGET_DIRECTORY]]' >&2
    exit 2
fi
out=$1
native=$2
busybox=${3:-/bin/busybox}
build_target=${4:-${CARGO_TARGET_DIR:-$out/cargo}}
[[ $out = /* && $native = /* && ! -e $out ]] || { echo 'output must be a new absolute directory; native build must be absolute' >&2; exit 2; }
[[ $build_target = /* ]] || { echo 'cargo target directory must be explicit and absolute' >&2; exit 2; }
for tool in cargo rustc cc python3 cpio gzip file readelf rg; do command -v "$tool" >/dev/null; done
file "$busybox" | rg -q 'statically linked'
for input in module/as_wait.ko as_hooks.bpf.o as-bpf-loader; do [[ -f $native/$input ]]; done
mkdir -- "$out"
guest=$out/root
mkdir -p -- "$guest"/{bin,proc,sys,dev,tmp,suite/driver,suite/files}
host=$(rustc -vV | sed -n 's/^host: //p')
cd -- "$repo"
# Explicit target keeps static linking flags away from host proc-macro shared libraries.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static --cfg getrandom_backend=\"linux_getrandom\"" cargo build --locked --offline -p as-driver-adapter --example native_smoke --target "$host" --target-dir "$build_target"
cp -- "$build_target/$host/debug/examples/native_smoke" "$guest/suite/native_smoke"
cc -static -O2 -Wall -Wextra -Werror scripts/native-smoke/subject.c -o "$guest/suite/subject"
cc -static -O2 -Wall -Wextra -Werror -I drivers/linux scripts/native-smoke/fd_loss.c -o "$guest/suite/fd_loss"
cp -- "$native/as-bpf-loader" "$guest/suite/driver/as-bpf-loader"
program_headers=$(readelf -l "$guest/suite/driver/as-bpf-loader")
[[ $program_headers != *INTERP* ]]
for binary in native_smoke subject fd_loss driver/as-bpf-loader; do
    program_headers=$(readelf -l "$guest/suite/$binary")
    [[ $program_headers != *INTERP* ]]
    file "$guest/suite/$binary"
done
cp -- "$native/as_hooks.bpf.o" "$guest/suite/driver/as_hooks.bpf.o"
cp -- "$native/module/as_wait.ko" "$guest/suite/as_wait.ko"
cp -- "$busybox" "$guest/bin/busybox"
for applet in sh mount mkdir insmod rmmod poweroff uname cat; do ln -s busybox "$guest/bin/$applet"; done
cp -- scripts/native-smoke/init "$guest/init"
chmod 0755 "$guest/init"
for name in allow deny held timeout unbound failure; do printf '%s\n' "$name" > "$guest/suite/files/$name"; done
printf '%s\n' 'runtime controls must not edit this input' > "$guest/suite/sentinel"
python3 - "$repo" "$guest" <<'PY'
import hashlib, json, pathlib, sys
repo, guest = map(pathlib.Path, sys.argv[1:])
paths = list((repo/'drivers/linux').glob('*.c')) + list((repo/'drivers/linux').glob('*.h'))
paths += [repo/'drivers/linux/Makefile',repo/'drivers/linux/build.sh']
paths += list((repo/'crates/backend/as-driver-adapter/native').glob('*.c')) + list((repo/'crates/backend/as-driver-adapter/native').glob('*.h'))
for crate in ['as-driver-adapter','as-types','as-kernel-common','as-process-tree']:
    paths += list((repo/'crates'/('contracts' if crate in ['as-types','as-kernel-common'] else 'backend')/crate).rglob('*.rs')) + [repo/'crates'/('contracts' if crate in ['as-types','as-kernel-common'] else 'backend')/crate/'Cargo.toml']
paths += list((repo/'scripts/native-smoke').glob('*'))
paths += [repo/'.cargo/config.toml',repo/'Cargo.toml',repo/'Cargo.lock']
source = {str(p.relative_to(repo)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths) if p.is_file()}
artifacts = {str(p.relative_to(guest)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((guest/'suite').rglob('*')) if p.is_file()}
(guest/'suite/source-sha256.json').write_text(json.dumps({'source':source,'artifacts':artifacts},sort_keys=True))
PY
(cd -- "$guest" && find . -print0 | cpio --null -o --owner=0:0 --format=newc --quiet | gzip -1) > "$out/initramfs.cpio.gz"
printf 'INITRAMFS=%s\nENTRY=/suite/native_smoke --guest-diagnostic /suite\nREQUIRED_CMDLINE=as_native_smoke=1\n' "$out/initramfs.cpio.gz"

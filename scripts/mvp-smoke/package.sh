#!/usr/bin/env bash
# Package the real scripted-service and middleware; never load anything on the host.
set -euo pipefail
umask 022
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
entry=middleware.py
if [[ ${1:-} == --frontend-ws ]]; then
    entry=frontend.py
    shift
fi
if [[ $# -lt 2 || $# -gt 4 ]]; then
    echo 'usage: package.sh [--frontend-ws] NEW_OUTPUT_DIRECTORY CURRENT_NATIVE_BUILD [STATIC_BUSYBOX [CARGO_TARGET_DIRECTORY]]' >&2
    exit 2
fi
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
out=$1
native=$2
busybox=${3:-/bin/busybox}
build_target=${4:-${CARGO_TARGET_DIR:-/tmp/as-refactor-target}}
[[ $out = /* && ! -e $out && $native = /* && $build_target = /* ]] || { echo 'new absolute output and explicit absolute native/target paths required' >&2; exit 2; }
for tool in cargo rustc cc python3 cpio gzip file readelf strip ldd modinfo zstd; do command -v "$tool" >/dev/null; done
file "$busybox" | rg -q 'statically linked'
for input in module/as_wait.ko as_hooks.bpf.o as_flow.bpf.o as-bpf-loader as-tls-ingress; do [[ -f $native/$input ]]; done
[[ -f $repo/crates/backend/as-main/examples/mvp_tls_fixture.rs ]]
[[ -f $repo/crates/middleware/as-middleware/src/bin/as-service.rs ]]
[[ -f $repo/scripts/mvp-smoke/middleware.py ]]
mkdir -m 0700 -- "$out"
guest=$out/root
mkdir -p -- "$guest"/{bin,proc,sys,dev,tmp,etc,suite/files,as/bin,as/config,as/driver,as/run,as/data,as/log}
chmod 1777 "$guest/tmp"
chmod 0755 "$guest/as" "$guest/as/bin" "$guest/as/config" "$guest/as/driver" "$guest/as/run"
chmod 0700 "$guest/as/data" "$guest/as/log"
cd -- "$repo"
host=$(rustc -vV | sed -n 's/^host: //p')
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static --cfg getrandom_backend=\"linux_getrandom\"" \
    cargo build --locked --offline -p as-main --example mvp_tls_fixture \
    --target "$host" --target-dir "$build_target"
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static --cfg getrandom_backend=\"linux_getrandom\"" \
    cargo build --locked --offline -p as-middleware --bin as-service \
    --target "$host" --target-dir "$build_target"
cp -- "$build_target/$host/debug/as-service" "$guest/as/bin/as-service"
cp -- "$build_target/$host/debug/examples/mvp_tls_fixture" "$guest/suite/mvp_tls_fixture"
if [[ $entry == frontend.py ]]; then
    RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static --cfg getrandom_backend=\"linux_getrandom\"" \
        cargo build --locked --offline -p as-middleware --example mvp_ws_client \
        --target "$host" --target-dir "$build_target"
    cp -- "$build_target/$host/debug/examples/mvp_ws_client" "$guest/suite/mvp_ws_client"
fi
for probe in subject rule_kill_probe self_probe; do
    cc -static -O2 -Wall -Wextra -Werror "$repo/scripts/native-smoke/$probe.c" -o "$guest/suite/$probe"
done
cp -- "$native/as-bpf-loader" "$guest/as/driver/as-bpf-loader"
program_headers=$(readelf -l "$guest/as/driver/as-bpf-loader")
[[ $program_headers != *INTERP* ]]
cp -- "$native/as-tls-ingress" "$guest/as/driver/as-tls-ingress"
program_headers=$(readelf -l "$guest/as/driver/as-tls-ingress")
[[ $program_headers != *INTERP* ]]
for binary in "$guest/as/bin/as-service" "$guest/suite/"*; do
    [[ -f $binary ]] || continue
    program_headers=$(readelf -l "$binary")
    [[ $program_headers != *INTERP* ]]
    strip --strip-debug "$binary"
done
cp -- "$native/module/as_wait.ko" "$guest/suite/as_wait.ko"
cp -- "$native/as_hooks.bpf.o" "$guest/as/driver/as_hooks.bpf.o"
cp -- "$native/as_flow.bpf.o" "$guest/as/driver/as_flow.bpf.o"
chmod go-w "$guest/as/bin/as-service" "$guest/as/driver/"*
cp -- scripts/mvp-smoke/middleware.py "$guest/suite/middleware.py"
if [[ $entry == frontend.py ]]; then
    cp -- scripts/mvp-smoke/frontend.py "$guest/suite/frontend.py"
fi
sed "s@/suite/middleware\.py run@/suite/$entry run@" scripts/mvp-smoke/guest-init > "$guest/init"
chmod 0755 "$guest/init"
cp -- "$busybox" "$guest/bin/busybox"
for applet in sh mount mkdir insmod rmmod poweroff uname cat grep sleep; do ln -s busybox "$guest/bin/$applet"; done
python3 - "$repo" "$guest" <<'PY'
import hashlib,json,pathlib,platform,shutil,subprocess,sys,sysconfig
repo,root=map(pathlib.Path,sys.argv[1:])
# The distro interpreter keeps its documented prefix layout; no pip/venv or user packages.
python=pathlib.Path(sys.executable).resolve()
stdlib=pathlib.Path(sysconfig.get_path('stdlib'))
if sys.base_prefix!='/usr' or not python.is_relative_to('/usr/bin') or not stdlib.is_relative_to('/usr/lib'):
    raise SystemExit('requires the installed distro /usr Python interpreter and standard library')
if platform.machine() not in ('aarch64','x86_64'):
    raise SystemExit('supported native packaging architectures: aarch64/x86_64')
def copy_file(path):
    path=pathlib.Path(path)
    dest=root/str(path).lstrip('/')
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(path,dest)
    return dest
copy_file(python)
(root/'usr/bin/python3').symlink_to(python.name)
shutil.copytree(stdlib,root/str(stdlib).lstrip('/'),ignore=shutil.ignore_patterns(
    '__pycache__','test','tests','site-packages','dist-packages','idlelib','tkinter','turtledemo','ensurepip'))
# ldd is used only on the installed trusted runtime ELF files, never client input.
seen=set()
def dependencies(path):
    path=pathlib.Path(path)
    if path in seen:return
    seen.add(path)
    result=subprocess.run(['ldd',str(path)],capture_output=True,text=True)
    if result.returncode:
        raise SystemExit('cannot determine runtime ELF dependencies: '+str(path))
    for line in result.stdout.splitlines():
        if 'not found' in line:raise SystemExit('missing runtime dependency: '+line)
        for token in line.split():
            if token.startswith('/'):
                library=pathlib.Path(token)
                if library not in seen:
                    copy_file(library)
                    dependencies(library)
copy_file('/usr/sbin/nft')
for binary in [python,pathlib.Path('/usr/sbin/nft'),*stdlib.rglob('*.so')]:
    if any(part in {'test','tests','site-packages','dist-packages','tkinter','idlelib'} for part in binary.relative_to('/usr').parts):
        continue
    dependencies(binary)
modules=[]
def module(name):
    if name in modules:return
    for dep in subprocess.check_output(['modinfo','-F','depends',name],text=True).strip().split(','):
        if dep:module(dep)
    path=pathlib.Path(subprocess.check_output(['modinfo','-F','filename',name],text=True).strip())
    if str(path)=='(builtin)':return
    dest=root/'lib/modules'/(name+'.ko');dest.parent.mkdir(parents=True,exist_ok=True)
    if path.suffix=='.zst':dest.write_bytes(subprocess.check_output(['zstd','-d','-c',str(path)]))
    else:shutil.copy2(path,dest)
    modules.append(name)
if platform.machine()=='aarch64':module('rtc-pl031')
for name in ['nf_tables','nf_conntrack','nf_nat','nft_chain_nat','nft_redir']:module(name)
(root/'suite/guest-kernel-modules').write_text(''.join('/lib/modules/'+name+'.ko\n' for name in modules))
config=(repo/'config/as.yaml').read_text()
if config.count('chat_content: false')!=1:raise SystemExit('unexpected source content-grant fixture')
config=config.replace('chat_content: false','chat_content: true')
(root/'as/config/as.yaml').write_text(config)
shutil.copyfile(repo/'config/rules.fast-allow.yaml',root/'as/config/rules.yaml')
for name in ['allow','deny','ask']:
    p=root/'suite/files'/name;p.write_text('original');p.chmod(0o666)
paths=[*repo.glob('Cargo.*'),repo/'.cargo/config.toml',repo/'config/as.yaml',repo/'config/rules.fast-allow.yaml']
paths.append(repo/'drivers/linux/Makefile')
for parent in ['crates','drivers/linux','scripts/mvp-smoke','scripts/native-smoke']:
    paths.extend(p for p in (repo/parent).rglob('*') if p.is_file() and p.suffix in {'.rs','.toml','.c','.h','.sh','.py','.md'})
paths.extend([repo/'scripts/mvp-smoke/guest-init',repo/'scripts/native-smoke/main-init'])
source={str(p.relative_to(repo)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(set(paths))}
(root/'suite/runtime.json').write_text(json.dumps({'architecture':platform.machine(),'kernel_release':platform.release(),'python':sys.version.split()[0],'python_executable':str(python),'stdlib':str(stdlib),'modules':modules},sort_keys=True)+'\n')
artifacts={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for parent in ['as','suite'] for p in sorted((root/parent).rglob('*')) if p.is_file()}
(root/'suite/source-mvp-sha256.json').write_text(json.dumps({'source':source,'artifacts':artifacts},sort_keys=True))
PY
(cd -- "$guest" && find . -print0 | cpio --null -o --owner=0:0 --format=newc --quiet | gzip -1) > "$out/initramfs.cpio.gz"
python3 - "$out" "$entry" <<'PY'
from pathlib import Path
import gzip,hashlib,json,stat,sys
p=Path(sys.argv[1]);image=p/'initramfs.cpio.gz'
# Read the actual GNU newc archive metadata, including its --owner=0:0 result.
raw=gzip.decompress(image.read_bytes());offset=0;protected=[]
while offset<len(raw):
    header=raw[offset:offset+110]
    if header[:6]!=b'070701':raise SystemExit('unexpected cpio encoding')
    values=[int(header[6+i*8:14+i*8],16) for i in range(13)]
    _,mode,uid,gid,_,_,size,_,_,_,_,namesize,_=values
    start=offset+110;name=raw[start:start+namesize-1].decode();offset=(start+namesize+3)&~3
    if name=='TRAILER!!!':break
    name=name.removeprefix('./')
    if name=='as' or name.startswith('as/'):
        protected.append({'path':name,'uid':uid,'gid':gid,'mode':oct(stat.S_IMODE(mode))})
        if uid!=0 or gid!=0 or mode&0o022:raise SystemExit('unsafe protected bundle archive metadata: '+name)
    offset=(offset+size+3)&~3
record={'image':str(image),'image_bytes':image.stat().st_size,'image_sha256':hashlib.sha256(image.read_bytes()).hexdigest(),'entry':'/suite/'+sys.argv[2]+' run','socket':'/as/run/middleware.sock','kernel_runtime':json.loads((p/'root/suite/runtime.json').read_text()),'protected_archive_metadata':protected}
(p/'package.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record))
PY

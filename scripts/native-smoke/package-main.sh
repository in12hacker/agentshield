#!/usr/bin/env bash
# Fresh formal MainAssembly guest; this builds bytes and does not load host modules.
set -euo pipefail
umask 022
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
if [[ $# -lt 2 || $# -gt 4 ]]; then
    echo 'usage: package-main.sh NEW_OUTPUT_DIRECTORY CURRENT_NATIVE_BUILD [STATIC_BUSYBOX [CARGO_TARGET_DIRECTORY]]' >&2
    exit 2
fi
out=$1
native=$2
busybox=${3:-/bin/busybox}
build_target=${4:-${CARGO_TARGET_DIR:-$out/cargo}}
[[ $out = /* && $native = /* && ! -e $out ]] || { echo 'new absolute output directory and absolute native build required' >&2; exit 2; }
[[ $build_target = /* ]] || { echo 'cargo target directory must be explicit and absolute' >&2; exit 2; }
for tool in cargo rustc cc python3 cpio gzip file readelf rg strip; do command -v "$tool" >/dev/null; done
file "$busybox" | rg -q 'statically linked'
for input in module/as_wait.ko as_hooks.bpf.o as_flow.bpf.o as-bpf-loader as-tls-ingress; do [[ -f $native/$input ]]; done
mkdir -- "$out"
guest=$out/root
mkdir -p -- "$guest"/{bin,proc,sys,dev,tmp,suite/files,as/bin,as/config,as/driver,as/run,as/data,as/log}
mkdir -p -- "$guest/suite/create-real" "$guest/suite/create-view" "$guest/suite/audit-fs/watch"
printf original > "$guest/suite/audit-fs/watch/underlying"
chmod 0777 "$guest/suite/create-real"
chmod 0700 "$guest/as/log"
host=$(rustc -vV | sed -n 's/^host: //p')
cd -- "$repo"
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static --cfg getrandom_backend=\"linux_getrandom\"" cargo build --locked --offline -p as-main --example native_service_smoke --example tls_pipeline_smoke --target "$host" --target-dir "$build_target"
cp -- "$build_target/$host/debug/examples/native_service_smoke" "$guest/as/bin/as-service"
cp -- "$guest/as/bin/as-service" "$guest/suite/callback_probe"
cp -- "$build_target/$host/debug/examples/tls_pipeline_smoke" "$guest/suite/tls_pipeline_smoke"
cc -static -O2 -Wall -Wextra -Werror scripts/native-smoke/subject.c -o "$guest/suite/subject"
cc -static -O2 -Wall -Wextra -Werror scripts/native-smoke/rule_kill_probe.c -o "$guest/suite/rule_kill_probe"
cc -static -O2 -Wall -Wextra -Werror scripts/native-smoke/self_probe.c -o "$guest/suite/self_probe"
cat > "$out/write_probe.c" <<'C'
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>
int main(int argc,char **argv){
    if(argc!=2)return 2;
    errno=0;int fd=open(argv[1],O_WRONLY|O_CLOEXEC);int error=fd<0?errno:0;
    if(fd>=0)close(fd);
    printf("{\"pid\":%ld,\"open_result\":%d,\"errno\":%d}\n",(long)getpid(),fd>=0?0:-1,error);return 0;
}
C
cc -static -O2 -Wall -Wextra -Werror "$out/write_probe.c" -o "$guest/suite/write_probe"
cat > "$out/umount_probe.c" <<'C'
#include <errno.h>
#include <stdio.h>
#include <sys/mount.h>
int main(int argc,char **argv){
    if(argc!=2)return 2;
    errno=0;int result=umount2(argv[1],0);int error=result<0?errno:0;
    printf("{\"result\":%d,\"errno\":%d}\n",result,error);return 0;
}
C
cc -static -O2 -Wall -Wextra -Werror "$out/umount_probe.c" -o "$guest/suite/umount_probe"
cp -- "$native/as-bpf-loader" "$guest/as/driver/as-bpf-loader"
program_headers=$(readelf -l "$guest/as/driver/as-bpf-loader")
[[ $program_headers != *INTERP* ]]
for binary in as/bin/as-service suite/subject suite/write_probe suite/umount_probe suite/self_probe suite/rule_kill_probe suite/callback_probe suite/tls_pipeline_smoke; do
    program_headers=$(readelf -l "$guest/$binary")
    [[ $program_headers != *INTERP* ]]
    file "$guest/$binary"
    strip --strip-debug "$guest/$binary"
done
cp -- "$native/as_hooks.bpf.o" "$guest/as/driver/as_hooks.bpf.o"
cp -- "$native/module/as_wait.ko" "$guest/suite/as_wait.ko"
cp -- "$native/as_flow.bpf.o" "$guest/as/driver/as_flow.bpf.o"
cp -- "$native/as-tls-ingress" "$guest/as/driver/as-tls-ingress"
program_headers=$(readelf -l "$guest/as/driver/as-tls-ingress")
[[ $program_headers != *INTERP* ]]
python3 - "$guest" <<'PY_TLS_ASSETS'
import pathlib,subprocess,shutil,sys
root=pathlib.Path(sys.argv[1])
for prog in ['/usr/sbin/nft']:
    dest=root/prog.lstrip('/');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(prog,dest)
    for line in subprocess.check_output(['ldd',prog],text=True).splitlines():
        for token in line.split():
            if token.startswith('/'):
                dest=root/token.lstrip('/');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(token,dest)
modules=[]
def module(name):
    if name in modules:return
    for dep in subprocess.check_output(['modinfo','-F','depends',name],text=True).strip().split(','):
        if dep:module(dep)
    path=pathlib.Path(subprocess.check_output(['modinfo','-F','filename',name],text=True).strip())
    dest=root/'lib/modules'/(name+'.ko');dest.parent.mkdir(parents=True,exist_ok=True)
    if path.suffix=='.zst':dest.write_bytes(subprocess.check_output(['zstd','-d','-c',str(path)]))
    else:shutil.copy2(path,dest)
    modules.append(name)
for name in ['rtc-pl031','nf_tables','nf_conntrack','nf_nat','nft_chain_nat','nft_redir']:module(name)
(root/'suite/guest-kernel-modules').write_text(''.join('/lib/modules/'+name+'.ko\n' for name in modules))
PY_TLS_ASSETS
cp -- config/as.yaml "$guest/as/config/as.yaml"
# This disposable guest intentionally grants its OS-authenticated observer content access.
python3 - "$guest/as/config/as.yaml" <<'PY_CHAT_FIXTURE'
from pathlib import Path
import sys
p=Path(sys.argv[1]);s=p.read_text();assert s.count('chat_content: false')==1
assert s.count('file_close_audit: []')==1
p.write_text(s.replace('chat_content: false','chat_content: true').replace('file_close_audit: []','file_close_audit: [/suite/audit-fs/watch]'))
PY_CHAT_FIXTURE
python3 - "$guest" <<'PY'
import json,pathlib,sys
root=pathlib.Path(sys.argv[1])
def rule(id,number,operations,action,target=None,source=None,log=False):
    return dict(id=id,number=number,enabled=True,scope={'kind':'all_protected'},operations=operations,source=source or {},target=target or {},action=action,log=log)
def group(kind,id,number,execution,rules):
    return dict(kind=kind,id=id,number=number,enabled=True,execution=execution,rules=rules)
def exact(value):return {'operator':'exact','value':value}
source={'executable':exact('/suite/subject')}
policy={'groups':[
    group('process','driver-process',2,{'location':'driver'},[rule('driver-execute',1,['execute'],{'type':'allow'})]),
    group('file','driver-file',3,{'location':'driver'},[
        rule('driver-deny',1,['open'],{'type':'deny','terminate':'none'},{'path':exact('/suite/files/deny')},source,True),
        rule('driver-allow',2,['open'],{'type':'allow'},{'path':exact('/suite/files/allow')},source,True),
        rule('driver-transfer',3,['open'],{'type':'to_user'},{'path':exact('/suite/files/ask')},source,True),
        rule('driver-other-open',4,['open'],{'type':'allow'})]),
    group('network','driver-network',2,{'location':'driver'},[rule('driver-network-allow',1,['connect','send'],{'type':'allow'})]),
    group('file','backend-file-ask',1,{'location':'backend','stage':'ask'},[
        rule('backend-ask',1,['open'],{'type':'ask','timeout_ms':1000,'timeout':{'type':'deny','terminate':'none'}},{'path':exact('/suite/files/ask')},source,True),
        rule('ask-create-open',2,['create_file'],{'type':'ask','timeout_ms':1000,'timeout':{'type':'deny','terminate':'none'}},{'path':exact('/suite/create-view/open-allow')},None,True),
        rule('ask-create-mknod',3,['create_file'],{'type':'ask','timeout_ms':1000,'timeout':{'type':'deny','terminate':'none'}},{'path':exact('/suite/create-view/mknod-deny')},None,True)])]}
# Native and backend paths share the same explicit scope model.
for name,scope in [('none','none'),('actor','actor'),('parent','actor_and_parent'),('protected','actor_and_parent')]:
    policy['groups'][1]['rules'].insert(0,rule('driver-kill-'+name,1,['open'],{'type':'deny','terminate':scope},{'path':exact('/suite/files/kill-driver-'+name)},{'executable':exact('/suite/subject' if name=='protected' else '/suite/rule_kill_probe')},log=True))
for number,r in enumerate(policy['groups'][1]['rules'],1):r['number']=number
for stage,name,scope in [('remembered','remembered','actor'),('fast','fast','actor_and_parent')]:
    policy['groups'].append(group('file','backend-kill-'+name,10 if stage=='remembered' else 11,{'location':'backend','stage':stage},[
        rule('backend-kill-'+name,1,['open'],{'type':'deny','terminate':scope},{'path':exact('/suite/files/kill-'+name)},{'executable':exact('/suite/rule_kill_probe')},log=True)]))
for name,scope in [('answer','none'),('timeout','actor_and_parent'),('disconnect','actor_and_parent')]:
    policy['groups'][3]['rules'].append(rule('ask-kill-'+name,len(policy['groups'][3]['rules'])+1,['open'],{'type':'ask','timeout_ms':1000,'timeout':{'type':'deny','terminate':scope}},{'path':exact('/suite/files/kill-'+name)},{'executable':exact('/suite/rule_kill_probe')},log=True))
# Only targets requiring backend matching are transferred; driver-only targets stay direct.
backend_names=['remembered','fast','answer','timeout','disconnect']
transfer=[rule('transfer-kill-'+name,1,['open'],{'type':'to_user'},{'path':exact('/suite/files/kill-'+name)},{'executable':exact('/suite/rule_kill_probe')},log=True) for name in backend_names]
policy['groups'][1]['rules'][0:0]=transfer
for number,r in enumerate(policy['groups'][1]['rules'],1):r['number']=number
for name in ['driver-none','driver-actor','driver-parent','driver-protected']+backend_names:
    target=root/'suite/files'/('kill-'+name);target.write_text('original');target.chmod(0o666)
# The same real source, compiler, native adoption and backend factory serve all families.
probe_source={'executable':exact('/suite/rule_kill_probe'),'uid':65534,'euid':65534}
backend_process=group('process','backend-process-smoke',100,{'location':'backend','stage':'fast'},[])
backend_network=group('network','backend-network-smoke',100,{'location':'backend','stage':'fast'},[])
policy['groups'] += [backend_process,backend_network]
new_process=[];new_network=[]
for operation,base,protocol in [('exec',0,None),('connect',18100,6),('send',18200,6),('sendto',18300,17)]:
    for stage,offset in [('driver',0),('fast',16)]:
        for index,(label,scope) in enumerate([('allow',None),('none','none'),('actor','actor'),('parent','actor_and_parent')]):
            name=f'{operation}-{stage}-{label}'
            action={'type':'allow'} if scope is None else {'type':'deny','terminate':scope}
            if operation=='exec':
                target={'executable':exact('/suite/'+name)}
                witness=root/'suite/files'/name;witness.write_text('original');witness.chmod(0o666)
                native_pool=new_process;backend_pool=backend_process['rules'];ops=['execute']
            else:
                port=base+offset+index
                target={'remote_cidr':'127.0.0.1/32','remote_port':{'first':port,'last':port},'protocol':protocol}
                native_pool=new_network;backend_pool=backend_network['rules'];ops=['connect' if operation=='connect' else 'send']
            if stage=='driver':native_pool.append(rule(name,1,ops,action,target,probe_source,True))
            else:
                native_pool.append(rule('transfer-'+name,1,ops,{'type':'to_user'},target,probe_source,True))
                backend_pool.append(rule(name,len(backend_pool)+1,ops,action,target,probe_source,True))
for pool,extra in [(policy['groups'][0]['rules'],new_process),(policy['groups'][2]['rules'],new_network)]:
    pool[0:0]=extra
    for number,r in enumerate(pool,1):r['number']=number
(root/'as/config/rules.yaml').write_text(json.dumps(policy,indent=2)+'\n')
PY
for stage in driver fast; do
    for action in allow none actor parent; do
        cp -- "$guest/suite/rule_kill_probe" "$guest/suite/exec-$stage-$action"
    done
done
cp -- "$busybox" "$guest/bin/busybox"
for applet in sh mount mkdir insmod rmmod poweroff uname cat grep; do ln -s busybox "$guest/bin/$applet"; done
cp -- scripts/native-smoke/main-init "$guest/init"
chmod 0755 "$guest/init"
for name in allow deny ask; do printf '%s\n' "$name" > "$guest/suite/files/$name"; done
python3 - "$repo" "$guest" <<'PY'
import hashlib,json,pathlib,sys
repo,guest=map(pathlib.Path,sys.argv[1:])
paths=list((repo/'drivers/linux').glob('*.c'))+list((repo/'drivers/linux').glob('*.h'))
paths += [repo/'drivers/linux/Makefile',repo/'drivers/linux/build.sh']
paths += list((repo/'crates/backend/as-driver-adapter/native').glob('*.c')) + list((repo/'crates/backend/as-driver-adapter/native').glob('*.h'))
for crate in ['as-main','as-types','as-driver-adapter','as-policy-engine','as-runtime-config','as-kernel-common','as-process-tree','as-flow','as-credentials','as-security-log']:
    paths+=list((repo/'crates'/('contracts' if crate in ['as-types','as-kernel-common'] else 'backend')/crate).rglob('*.rs'))
    paths.append(repo/'crates'/('contracts' if crate in ['as-types','as-kernel-common'] else 'backend')/crate/'Cargo.toml')
paths+=list((repo/'scripts/native-smoke').glob('*'))
paths += [repo/'.cargo/config.toml',repo/'Cargo.toml',repo/'Cargo.lock']
sources={str(p.relative_to(repo)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths) if p.is_file()}
artifacts={str(p.relative_to(guest)):hashlib.sha256(p.read_bytes()).hexdigest() for parent in ['as','suite'] for p in sorted((guest/parent).rglob('*')) if p.is_file()}
(guest/'suite/source-main-sha256.json').write_text(json.dumps({'source':sources,'artifacts':artifacts},sort_keys=True))
PY
(cd -- "$guest" && find . -print0 | cpio --null -o --owner=0:0 --format=newc --quiet | gzip -1) > "$out/initramfs.cpio.gz"
printf 'INITRAMFS=%s\nENTRY=/as/bin/as-service --guest-main-smoke\nREQUIRED_CMDLINE=as_native_main_smoke=1\n' "$out/initramfs.cpio.gz"

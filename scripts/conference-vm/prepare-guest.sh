#!/bin/bash
# Run as root inside the owned Debian guest, never on the workstation.
set -euo pipefail
[[ $(uname -m) == x86_64 && -d /mnt/payload/drivers/linux ]]
[[ -f /var/lib/as-packages-ready && $(systemd-detect-virt) == qemu ]]
[[ $# == 1 && $1 == /* ]]
native=$(realpath -e -- "$1")
for asset in module/as_wait.ko as-bpf-loader as-tls-ingress as_hooks.bpf.o as_flow.bpf.o; do
    [[ -f $native/$asset ]]
done
[[ $(modinfo -F vermagic "$native/module/as_wait.ko") == "$(uname -r) "* ]]
src=/mnt/payload
root=/opt/agentshield
install -d -m 0755 "$root"/{bin,config,driver,run} /opt/as-conference
install -d -m 0700 "$root"/{data,log} /var/lib/as-conference
# The distributed kernel already supplies Docker bridge's veth driver.
install -d /etc/modules-load.d
printf 'veth\nnft_redir\nxt_tcpudp\nxt_nat\n' >/etc/modules-load.d/as-conference-docker.conf
install -m 0755 "$src/as-service" "$root/bin/as-service"
install -m 0644 "$src/config/as.yaml" "$root/config/as.yaml"
install -m 0644 "$src/config/rules.fast-allow.yaml" "$root/config/rules.yaml"
install -m 0644 "$src/config/rules.conference.yaml" /opt/as-conference/rules.conference.yaml
install -m 0644 "$src/LICENSE" /opt/as-conference/AS-LICENSE
install -m 0755 "$native"/as-{bpf-loader,tls-ingress} "$root/driver/"
install -m 0644 "$native"/as_{hooks,flow}.bpf.o "$root/driver/"
install -D -m 0644 "$native/module/as_wait.ko" "/lib/modules/$(uname -r)/extra/as_wait.ko"
depmod -a
install -d /var/www/agentshield
cp -a "$src/ui/dashboard/dist/." /var/www/agentshield/
cat >/etc/nginx/sites-available/agentshield <<'NGINX'
server {
    listen 127.0.0.1:8080;
    root /var/www/agentshield;
    location / { try_files $uri $uri/ /index.html; }
}
NGINX
ln -sf /etc/nginx/sites-available/agentshield /etc/nginx/sites-enabled/agentshield
rm -f /etc/nginx/sites-enabled/default
id demo >/dev/null 2>&1 || useradd -m -s /bin/bash demo
usermod -a -G sudo demo
printf 'demo ALL=(ALL) NOPASSWD:ALL\n' >/etc/sudoers.d/as-conference
chmod 0440 /etc/sudoers.d/as-conference
passwd -l demo
install -d /etc/lightdm/lightdm.conf.d /home/demo/.config/autostart /home/demo/Desktop
cat >/etc/systemd/network/05-as-conference.network <<'NETWORK'
[Match]
Name=en* eth*
[Network]
DHCP=yes
IPv6AcceptRA=yes
NETWORK
cat >/etc/lightdm/lightdm.conf.d/50-as-conference.conf <<'LIGHTDM'
[Seat:*]
autologin-user=demo
autologin-user-timeout=0
user-session=xfce
LIGHTDM
install -m 0755 "$src/scripts/conference-vm/first-boot.py" /opt/as-conference/first-boot.py
install -m 0755 "$src/scripts/conference-vm/demo.py" /opt/as-conference/demo.py
install -m 0644 "$src/scripts/conference-vm/DEMO-TEST-CASES.en.md.template" /opt/as-conference/DEMO-TEST-CASES.en.md.template
install -m 0755 "$src/scripts/conference-vm/bootstrap-flow.py" /opt/as-conference/bootstrap-flow.py
install -m 0755 "$src/scripts/conference-vm/openclaw-phantom.py" /opt/as-conference/openclaw-phantom.py
install -m 0644 "$src/downloads/openclaw-image.json" /opt/as-conference/openclaw-image.json
install -m 0644 "$src/scripts/conference-vm/compose.yaml" /opt/as-conference/compose.yaml
python3 /opt/as-conference/demo.py --prepare-site
demo_port=$(python3 /opt/as-conference/demo.py --port)
cat >/etc/systemd/system/as-conference-demo-http.service <<UNIT
[Unit]
Description=Local AgentShield TCP Connect demonstration website
After=network.target
Before=openclaw.service
[Service]
User=demo
Group=demo
ExecStart=/usr/bin/python3 -m http.server $demo_port --bind 0.0.0.0 --directory /opt/as-conference/demo-http
TimeoutStopSec=infinity
[Install]
WantedBy=multi-user.target
UNIT
image=$(python3 -c 'import json;print(json.load(open("/opt/as-conference/openclaw-image.json"))["local_id"])')
if ! docker image inspect "$image" >/dev/null 2>&1; then
    docker load -i "$src/openclaw-image.tar"
fi
[[ $(docker image inspect -f '{{.Id}}' "$image") == "$image" ]]
docker image inspect "$image" >/opt/as-conference/openclaw-inspect.json
python3 - <<'PY'
import json, pathlib, subprocess
p = pathlib.Path('/opt/as-conference/openclaw-image.json')
record = json.loads(p.read_text())
for field, option in [('runtime_uid', '-u'), ('runtime_gid', '-g')]:
    record[field] = int(subprocess.check_output([
        'docker', 'run', '--rm', '--entrypoint', '/usr/bin/id', record['local_id'], option,
    ], text=True).strip())
p.write_text(json.dumps(record, indent=2) + '\n')
PY
cat >/etc/systemd/system/as-conference-first-boot.service <<'UNIT'
[Unit]
Description=Initialize this conference appliance's local identities
Before=agentshield.service openclaw.service lightdm.service
After=local-fs.target
ConditionPathExists=!/var/lib/as-conference/initialized
[Service]
Type=oneshot
ExecStart=/opt/as-conference/first-boot.py
RemainAfterExit=yes
[Install]
WantedBy=multi-user.target
UNIT
cat >/etc/systemd/system/agentshield.service <<'UNIT'
[Unit]
Description=AgentShield backend and middleware
Requires=as-conference-first-boot.service
After=as-conference-first-boot.service as-conference-bootstrap.service network-online.target
Wants=network-online.target as-conference-bootstrap.service
Before=openclaw.service lightdm.service
[Service]
ExecStartPre=/sbin/modprobe as_wait
ExecStart=/opt/agentshield/bin/as-service
ExecStartPost=/opt/as-conference/bootstrap-flow.py --wait-ready $MAINPID
KillMode=process
TimeoutStartSec=infinity
TimeoutStopSec=infinity
[Install]
WantedBy=multi-user.target
UNIT
cat >/etc/systemd/system/as-conference-bootstrap.service <<'UNIT'
[Unit]
Description=Provision this clean appliance through the AS management interface
After=as-conference-first-boot.service docker.service
Wants=docker.service
Requires=as-conference-first-boot.service
Before=agentshield.service openclaw.service lightdm.service
ConditionPathExists=!/var/lib/as-conference/bootstrap-complete
[Service]
Type=oneshot
ExecStartPre=/sbin/modprobe as_wait
ExecStart=/opt/as-conference/bootstrap-flow.py
KillMode=process
TimeoutStartSec=infinity
TimeoutStopSec=infinity
UNIT
cat >/etc/systemd/system/openclaw.service <<'UNIT'
[Unit]
Description=Official OpenClaw Docker gateway
Requires=docker.service agentshield.service as-conference-first-boot.service
After=docker.service agentshield.service as-conference-first-boot.service
[Service]
EnvironmentFile=/var/lib/as-conference/openclaw.env
ExecStart=/usr/bin/docker-compose --file /opt/as-conference/compose.yaml up --no-build --no-color --exit-code-from openclaw --timeout -1
ExecStop=/usr/bin/docker-compose --file /opt/as-conference/compose.yaml stop --timeout -1
TimeoutStopSec=infinity
[Install]
WantedBy=multi-user.target
UNIT
cat >/home/demo/.config/autostart/as-welcome.desktop <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=AgentShield welcome
Exec=firefox-esr /home/demo/Desktop/START-HERE.html
DESKTOP
cat >/home/demo/Desktop/openclaw-phantom.desktop <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=配置 OpenClaw 幻影 Key
Name[en]=Configure OpenClaw phantom key
Exec=/opt/as-conference/openclaw-phantom.py
Icon=dialog-password
Terminal=false
DESKTOP
chmod 0755 /home/demo/Desktop/openclaw-phantom.desktop
chown -R demo:demo /home/demo/.config /home/demo/Desktop
install -d /etc/systemd/system/serial-getty@ttyS0.service.d
cat >/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf <<'GETTY'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin demo --noclear %I $TERM
GETTY
systemctl enable as-conference-first-boot.service agentshield.service openclaw.service lightdm.service nginx.service as-conference-demo-http.service
# Final image has no unattended network shell; builder SSH is retired on sealing.
printf 'AS_CONFERENCE_PREPARED\n'

#!/bin/bash
# Execute as a transient root unit after the builder SSH connection has exited.
set -euo pipefail
[[ $(systemd-detect-virt) == qemu && -f /var/lib/as-packages-ready ]]
[[ -x /opt/agentshield/bin/as-service && -f /etc/systemd/system/openclaw.service ]]
# This disk is the clean master. Runtime verification happens in a separate clone.
[[ ! -e /var/lib/as-conference/initialized && ! -e /var/lib/as-conference/local-identity.json ]]
[[ ! -e /home/demo/.openclaw && ! -e /opt/agentshield/data/middleware/business.sqlite ]]
systemctl stop ssh.service ssh.socket
systemctl disable ssh.service ssh.socket
if getent passwd builder >/dev/null; then
    builder_uid=$(id -u builder)
    # Wait on the original systemd slice before deleting that build account.
    systemctl stop "user-$builder_uid.slice"
    userdel -r builder
fi
rm -f /etc/sudoers.d/90-cloud-init-users
cloud-init clean --logs --seed --machine-id
touch /etc/cloud/cloud-init.disabled
rm -f /etc/ssh/ssh_host_* /etc/netplan/50-cloud-init.yaml
rm -f /var/lib/dbus/machine-id
ln -s /etc/machine-id /var/lib/dbus/machine-id
truncate -s 0 /etc/machine-id
umount /mnt/payload
rm -rf /var/lib/as-conference/native /var/lib/as-conference/driver-source
rm -f /var/lib/as-conference/prepare-guest.sh /var/lib/as-conference/prepare-guest-assembly.sh
apt-get clean
journalctl --rotate
journalctl --vacuum-time=1s
printf 'SEALED_CLEAN_MASTER\n' >/opt/as-conference/master-status
sync
fstrim -av
systemctl poweroff

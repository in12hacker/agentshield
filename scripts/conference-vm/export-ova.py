#!/usr/bin/env python3
"""Export a stopped, sealed guest with standard OVF 1.0 and a sparse VMDK."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import xml.etree.ElementTree as ET

OVF = 'http://schemas.dmtf.org/ovf/envelope/1'
RASD = 'http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_ResourceAllocationSettingData'
VSSD = 'http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_VirtualSystemSettingData'
VBOX = 'http://www.virtualbox.org/ovf/machine'


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def export(source, destination):
    destination.mkdir(parents=True, exist_ok=True)
    ova = destination / 'AgentShield-Conference-x86_64.ova'
    if ova.exists():
        raise FileExistsError('published OVA already exists: ' + str(ova))
    # Both check and convert take QEMU image locks; an active writer fails.
    subprocess.run(['qemu-img', 'check', str(source)], check=True)
    info = json.loads(subprocess.check_output(['qemu-img', 'info', '--output=json', str(source)]))
    disk = destination / 'agentshield-disk.vmdk'
    if not disk.exists():
        subprocess.run(['qemu-img', 'convert', '-p', '-O', 'vmdk', '-o',
                        'subformat=streamOptimized,adapter_type=ide', str(source), str(disk)], check=True)
    disk_info = json.loads(subprocess.check_output(['qemu-img', 'info', '--output=json', str(disk)]))
    if disk_info['format'] != 'vmdk' or disk_info['format-specific']['data']['create-type'] != 'streamOptimized':
        raise ValueError('expected the streamOptimized intermediate VMDK')
    subprocess.run(['qemu-img', 'compare', str(source), str(disk)], check=True)
    for prefix, namespace in [('ovf', OVF), ('rasd', RASD), ('vssd', VSSD), ('vbox', VBOX)]:
        ET.register_namespace(prefix, namespace)

    def node(parent, tag, value=None, namespace=OVF, **attributes):
        element = ET.SubElement(parent, '{' + namespace + '}' + tag,
                                {'{' + OVF + '}' + key: str(val) for key, val in attributes.items()})
        if value is not None:
            element.text = str(value)
        return element

    root = ET.Element('{' + OVF + '}Envelope')
    references = node(root, 'References')
    node(references, 'File', id='disk-file', href=disk.name, size=disk.stat().st_size)
    section = node(root, 'DiskSection')
    node(section, 'Info', 'Boot disk')
    node(section, 'Disk', diskId='boot', fileRef='disk-file', capacity=info['virtual-size'],
         capacityAllocationUnits='byte',
         format='http://www.vmware.com/interfaces/specifications/vmdk.html#streamOptimized')
    network = node(root, 'NetworkSection')
    node(network, 'Info', 'NAT network')
    node(node(network, 'Network', name='NAT'), 'Description', 'NAT')
    system = node(root, 'VirtualSystem', id='AgentShield')
    node(system, 'Info', 'AgentShield and official Docker OpenClaw')
    node(system, 'Name', 'AgentShield Conference')
    operating = node(system, 'OperatingSystemSection', id='96')
    operating.set('{' + VBOX + '}OSType', 'Debian_64')
    node(operating, 'Info', '64-bit Debian Linux guest')
    node(operating, 'Description', 'Debian_64')
    hardware = node(system, 'VirtualHardwareSection')
    node(hardware, 'Info', '4 vCPU, 8 GiB RAM, SATA, E1000, NAT, BIOS')
    settings = node(hardware, 'System')
    for name, value in [('ElementName', 'Virtual hardware'), ('InstanceID', '0'),
                        ('VirtualSystemIdentifier', 'AgentShield'), ('VirtualSystemType', 'virtualbox-2.2')]:
        node(settings, name, value, VSSD)

    def item(identifier, kind, title, **fields):
        element = node(hardware, 'Item')
        for name, value in {'InstanceID': identifier, 'ResourceType': kind,
                            'ElementName': title, **fields}.items():
            node(element, name, value, RASD)

    item(1, 3, '4 virtual CPUs', VirtualQuantity=4)
    item(2, 4, '8192 MiB memory', VirtualQuantity=8192, AllocationUnits='byte * 2^20')
    item(3, 20, 'SATA controller', ResourceSubType='AHCI', Address=0)
    item(4, 17, 'Boot disk', HostResource='ovf:/disk/boot', Parent=3, AddressOnParent=0)
    item(5, 10, 'Network adapter', ResourceSubType='E1000', Connection='NAT',
         AutomaticAllocation='true', AddressOnParent=0)
    ovf = destination / 'agentshield.ovf'
    ET.indent(root)
    ET.ElementTree(root).write(ovf, encoding='utf-8', xml_declaration=True)
    manifest = destination / 'agentshield.mf'
    manifest.write_text(''.join('SHA256 (' + f.name + ') = ' + sha(f) + '\n' for f in [ovf, disk]))
    with tarfile.open(ova, 'w', format=tarfile.USTAR_FORMAT) as archive:
        for file in [ovf, manifest, disk]:
            archive.add(file, arcname=file.name, recursive=False)
    (destination / 'SHA256SUMS').write_text(sha(ova) + '  ' + ova.name + '\n')
    print(json.dumps({'ova': str(ova), 'sha256': sha(ova), 'bytes': ova.stat().st_size,
                      'virtual_disk_bytes': info['virtual-size']}, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('sealed_disk', type=Path)
    parser.add_argument('new_output_directory', type=Path)
    args = parser.parse_args()
    export(args.sealed_disk.resolve(strict=True), args.new_output_directory.resolve())

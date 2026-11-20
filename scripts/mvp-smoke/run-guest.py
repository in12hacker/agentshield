#!/usr/bin/env python3
"""Run one packaged scripted MVP in an ordinary-UID QEMU; preserve raw evidence."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


def sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    parser.add_argument('--kernel', type=Path, required=True,
                        help='readable uncompressed Image/bzImage matching the packaged modules')
    parser.add_argument('--output', type=Path,
                        help='new host evidence directory (default: PACKAGE/run)')
    parser.add_argument('--timeout', type=int, default=300)
    args = parser.parse_args()
    package = args.package.resolve()
    output = args.output.resolve() if args.output else package / 'run'
    if not 1 <= args.timeout <= 3600:
        parser.error('timeout must be 1..3600 seconds')
    output.mkdir(mode=0o700, parents=False, exist_ok=False)
    result = {'passed': False, 'execution_host_uid': os.getuid(),
              'guest_authority': 'real root inside QEMU', 'broker_execution': False,
              'qemu_child_joined': False, 'timed_out': False}
    failures = []
    guest_report = None
    start = time.monotonic()
    try:
        record = json.loads((package / 'package.json').read_text())
        image = package / 'initramfs.cpio.gz'
        if image.stat().st_size != record['image_bytes'] or sha256(image) != record['image_sha256']:
            raise ValueError('packaged initramfs does not match package.json')
        kernel = args.kernel.resolve(strict=True)
        if not kernel.is_file():
            raise ValueError('kernel must be an existing regular file')
        arch = record['kernel_runtime']['architecture']
        if arch == 'aarch64':
            machine = ['/usr/bin/qemu-system-aarch64', '-machine', 'virt']
            console = 'ttyAMA0'
        elif arch == 'x86_64':
            machine = ['/usr/bin/qemu-system-x86_64', '-machine', 'q35']
            console = 'ttyS0'
        else:
            raise ValueError('unsupported packaged architecture')
        command = machine + ['-cpu', 'max', '-accel', 'tcg,thread=multi', '-smp', '2',
            '-m', '1024', '-nographic', '-no-reboot', '-kernel', str(kernel),
            '-initrd', str(image), '-append',
            f'console={console} rdinit=/init as_mvp_smoke=1 nokaslr '
            'lsm=lockdown,capability,landlock,yama,apparmor,bpf,ima,evm']
        if record['entry'] == '/suite/frontend.py run':
            command += ['-nic', 'none']
        result.update(argv=command, entry=record['entry'], kernel_sha256=sha256(kernel),
                      image_sha256=record['image_sha256'])
        try:
            completed = subprocess.run(command, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, timeout=args.timeout, check=False)
            raw, result['qemu_exit'] = completed.stdout, completed.returncode
        except subprocess.TimeoutExpired as error:
            raw = error.stdout or b''
            result.update(qemu_exit=124, timed_out=True)
            failures.append('QEMU deadline expired')
        # subprocess.run kills and waits on its own QEMU child before raising TimeoutExpired.
        result['qemu_child_joined'] = True
        (output / 'qemu-full.log').write_bytes(raw)
        text = raw.decode(errors='replace').replace('\r\n', '\n')
        blocks = re.findall(r'^AS_MVP_REPORT_BEGIN\n(.*?)\nAS_MVP_REPORT_END$',
                            text, flags=re.MULTILINE | re.DOTALL)
        if len(blocks) != 1:
            failures.append('guest did not publish exactly one middleware report')
        else:
            guest_report = json.loads(blocks[0])
            (output / 'middleware-report.json').write_text(
                json.dumps(guest_report, ensure_ascii=False, indent=2) + '\n')
            cases = guest_report.get('cases')
            if guest_report.get('passed') is not True or not isinstance(cases, list) or not cases:
                failures.append('middleware report did not establish passing cases')
            else:
                optional = []
                for case in cases:
                    if not isinstance(case, dict):
                        failures.append('middleware case is not an object')
                    elif case.get('required', True) is False:
                        optional.append({'name': case.get('name'), 'status': case.get('status')})
                    elif case.get('status') != 'PASS':
                        failures.append('required middleware case did not PASS: ' + str(case.get('name')))
                result['optional_cases'] = optional
        exits = re.findall(r'^AS_MVP_GUEST_EXIT=(\d+) MODULE_UNLOADED=(\d+)$', text, re.MULTILINE)
        if len(exits) != 1:
            failures.append('guest terminal exit record missing or duplicated')
        else:
            result.update(guest_exit=int(exits[0][0]), native_module_unloaded=exits[0][1] == '1')
            if exits[0] != ('0', '1'):
                failures.append('middleware/guest exit or actual Native unload failed')
        if result.get('qemu_exit') != 0:
            failures.append('QEMU exited unsuccessfully')
        if 'AS_MVP_PYTHON_READY' not in text:
            failures.append('packaged Python runtime did not initialize')
        result['passed'] = not failures
    except (OSError, ValueError, KeyError, TypeError) as error:
        failures.append(f'{type(error).__name__}: {error}')
    result.update(elapsed_seconds=round(time.monotonic() - start, 2), failures=failures,
                  middleware_report='middleware-report.json' if guest_report is not None else None)
    (output / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'passed': result['passed'], 'output': str(output), 'failures': failures}))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

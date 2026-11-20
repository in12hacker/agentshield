#!/usr/bin/python3
"""Configure the preset GLM connection through stock CLI; keep keys out of argv."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

CONFIGURE = r'''
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const cache = path.join(process.env.HOME, '.cache', 'openclaw');
fs.mkdirSync(cache, {recursive:true, mode:0o700});
const directory = fs.mkdtempSync(path.join(cache, 'as-config-'));
try {
    const input = path.join(directory, 'batch.json');
    fs.writeFileSync(input, fs.readFileSync(0), {mode:0o600, flag:'wx'});
    const result = spawnSync(process.execPath,
        ['dist/index.js', 'config', 'set', '--batch-file', input, '--strict-json'],
        {stdio:['ignore', 'inherit', 'inherit']});
    if (result.error) throw result.error;
    if (result.signal) console.error('OpenClaw config ended by signal:', result.signal);
    process.exitCode = result.status === null ? 1 : result.status;
} finally {
    fs.rmSync(directory, {recursive:true});
}
'''


def client_preset():
    """One public GLM connection preset for first boot and credential setup."""
    name, model = 'as-vm-glm', 'glm-5.3-flash'
    reference = name + '/' + model
    provider = {
        'baseUrl': 'https://open.bigmodel.cn/api/v1', 'api': 'openai-responses',
        'auth': 'api-key', 'headers': {'Connection': 'close'},
        'models': [{'id': model, 'name': 'GLM', 'input': ['text']}],
    }
    defaults = {'model': {'primary': reference},
        'models': {reference: {'params': {'transport': 'sse'}}}}
    return name, provider, defaults


def run_managed(command, *, input=None):
    result = subprocess.run(command, input=input, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode != 0:
        failure = Path(tempfile.mkdtemp(prefix='openclaw-config-', dir='/var/lib/as-conference'))
        for name, content in [('stdout', result.stdout), ('stderr', result.stderr)]:
            descriptor = os.open(failure / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            os.fchmod(descriptor, 0o600)
            with os.fdopen(descriptor, 'wb') as output:
                output.write(content)
        raise RuntimeError('OpenClaw managed command failed: exit ' + str(result.returncode)
            + '\nOriginal output: ' + str(failure) + '\n' + result.stderr.decode(errors='replace'))
    return result


def apply():
    if os.geteuid() != 0:
        raise RuntimeError('local administrator permission is required')
    value = sys.stdin.buffer.read()
    # This is the existing AS token wire shape, not a network/authentication preflight.
    if len(value) != 49 or not value.startswith(b'as_ph_'):
        raise RuntimeError('paste an AS-issued phantom key, not a provider key')
    token = value.decode('ascii')
    compose = ['docker-compose', '--env-file', '/var/lib/as-conference/openclaw.env',
               '--file', '/opt/as-conference/compose.yaml']
    selected = subprocess.check_output(compose + ['ps', '--all', '--quiet', 'openclaw'], text=True).splitlines()
    if len(selected) != 1:
        raise RuntimeError('the managed OpenClaw container is unavailable')
    container = subprocess.check_output(['docker', 'inspect', '--format', '{{.Id}}', selected[0]], text=True).strip()
    state = json.loads(subprocess.check_output(
        ['docker', 'inspect', '--format', '{{json .State}}', container], text=True))
    if not state['Running']:
        run_managed(compose + ['start', 'openclaw'])
        state = json.loads(subprocess.check_output(
            ['docker', 'inspect', '--format', '{{json .State}}', container], text=True))
        if not state['Running']:
            raise RuntimeError('the same OpenClaw container did not remain running: ' + json.dumps(state))
    info = json.loads(Path('/opt/as-conference/openclaw-image.json').read_text())
    name, provider, defaults = client_preset()
    reference = defaults['model']['primary']
    provider.update(apiKey=token)
    batch = json.dumps([
        {'path': 'models.providers.' + name, 'value': provider},
        {'path': 'agents.defaults.model.primary', 'value': reference},
        {'path': 'agents.defaults.models[' + json.dumps(reference) + '].params.transport',
         'value': defaults['models'][reference]['params']['transport']},
    ]).encode()
    result = run_managed(['docker', 'exec', '-i', '--user',
        str(info['runtime_uid']) + ':' + str(info['runtime_gid']), container,
        'node', '-e', CONFIGURE],
        input=batch)
    sys.stdout.buffer.write(result.stdout)
    sys.stderr.buffer.write(result.stderr)


def main():
    if sys.argv[1:] == ['--apply']:
        try:
            apply()
        except Exception as error:
            print(str(error), file=sys.stderr)
            return 1
        return 0
    if not sys.argv[1:]:
        return subprocess.call(['xterm', '-title', 'OpenClaw Phantom Key', '-geometry', '100x20',
            '-xrm', '*VT100.Translations: #override\\n'
            ' Ctrl Shift <Key>V: insert-selection(CLIPBOARD)\\n'
            ' Shift <Key>Insert: insert-selection(CLIPBOARD)',
            '-e', sys.executable, str(Path(__file__).resolve()), '--prompt'])
    if sys.argv[1:] != ['--prompt']:
        print('Usage: openclaw-phantom.py [--prompt | --apply]', file=sys.stderr)
        return 2
    print('OpenClaw 幻影 Key 配置 / Configure phantom key')
    print('输入明文可见。粘贴：Ctrl+Shift+V 或 Shift+Insert。')
    print('请从本虚拟机的 AS 页面复制幻影 Key，不要输入模型商的真实 Key。')
    try:
        value = input('Phantom Key: ').encode()
    except (EOFError, KeyboardInterrupt):
        print('\n已取消，未提交配置。')
        return 1
    print('正在保存 GLM 幻影 Key 连接 / Saving the GLM phantom-key connection…', flush=True)
    saved = subprocess.run(['sudo', '-n', sys.executable, str(Path(__file__).resolve()), '--apply'],
        input=value, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    sys.stdout.buffer.write(saved.stdout)
    sys.stderr.buffer.write(saved.stderr)
    if saved.returncode == 0:
        print('OK — GLM Provider 与幻影 Key 已通过官方 CLI 保存。\n重载状态见上方 CLI 提示；请发送聊天验证模型回复。')
    else:
        print('配置失败，原始错误见上方。', file=sys.stderr)
    try:
        input('Press Enter to close / 按回车关闭窗口…')
    except (EOFError, KeyboardInterrupt):
        pass
    return saved.returncode


if __name__ == '__main__':
    raise SystemExit(main())

#!/usr/bin/python3
"""Public demo resources, derived from the one conference rule template."""
import html
import ipaddress
import json
import os
from pathlib import Path
import pwd
import shlex
import subprocess
import sys
import yaml

ROOT = Path('/opt/as-conference')
GATEWAY_CIDR = '@docker_gateway_ipv4@/32'


def template():
    return yaml.safe_load((ROOT / 'rules.conference.yaml').read_text())


def targets(document=None):
    rules = {group['kind']: group['rules'][0] for group in (document or template())['groups'] if group['id'] in ['conference-process-review', 'conference-file-review', 'conference-network-review']}
    process = rules['process']['target']['executable']['value']
    file = rules['file']['target']['path']['value']
    port = rules['network']['target']['remote_port']
    if port['first'] != port['last']:
        raise ValueError('The local demo website needs one configured TCP port')
    return process, file, port['first']


def prepare_site():
    site = ROOT / 'demo-http'
    site.mkdir(exist_ok=True)
    site.chmod(0o755)
    (site / 'index.html').write_text('AS_NETWORK_DEMO_OK\n')
    (site / 'index.html').chmod(0o644)
    seed = ROOT / 'demo-files'
    seed.mkdir(exist_ok=True)
    seed.chmod(0o755)
    (seed / 'ask-file.txt').write_text('AS_FILE_DEMO_OK\n')
    (seed / 'ask-file.txt').chmod(0o644)


def prepare_file(image_info):
    _, target, _ = targets()
    compose = yaml.safe_load((ROOT / 'compose.yaml').read_text())
    for volume in compose['services']['openclaw']['volumes']:
        host, container, *_ = volume.split(':')
        relative = Path(target).relative_to(container) if Path(target).is_relative_to(container) else None
        if relative is None:
            continue
        destination = Path(host) / relative
        current = Path(host)
        for part in relative.parts[:-1]:
            current /= part
            if not current.exists():
                current.mkdir(mode=0o755)
                os.chown(current, image_info['runtime_uid'], image_info['runtime_gid'])
        if not destination.exists():
            with destination.open('x') as output:
                output.write((ROOT / 'demo-files/ask-file.txt').read_text())
            destination.chmod(0o644)
            os.chown(destination, image_info['runtime_uid'], image_info['runtime_gid'])
        return
    raise RuntimeError('The demo file has no actual Compose bind mapping')


def gateway(container):
    info = json.loads(subprocess.check_output(['docker', 'inspect', container], text=True))[0]
    addresses = set()
    for name, attachment in info['NetworkSettings']['Networks'].items():
        network_id = attachment.get('NetworkID')
        network = json.loads(subprocess.check_output(
            ['docker', 'network', 'inspect', network_id or name], text=True))[0]
        if network_id and network['Id'] != network_id:
            raise RuntimeError('Docker returned a different attached network identity')
        if network['Driver'] != 'bridge':
            continue
        for subnet in network['IPAM']['Config']:
            value = subnet.get('Gateway')
            if value and ipaddress.ip_address(value).version == 4:
                addresses.add(str(ipaddress.ip_address(value)))
    if len(addresses) != 1:
        raise RuntimeError('Docker did not provide one attached bridge IPv4 gateway')
    return addresses.pop()


def write_tasks(address):
    process, file, port = targets()
    url = f'http://{ipaddress.IPv4Address(address)}:{port}/'
    file_js = ('const fs=require("node:fs");const p=' + json.dumps(file)
               + ';process.stdout.write(fs.readFileSync(p,"utf8"));'
               + 'fs.writeFileSync(p,"AS_FILE_DEMO_OK\\n");')
    dns_rule = next(group['rules'][0] for group in template()['groups'] if group['id'] == 'conference-dns-review')
    dns_domain = dns_rule['target']['domain']['value']
    dns_url = 'https://' + dns_domain + '/'
    commands = [
        ('进程 Execute', process + " 'AS_PROCESS_DEMO_OK\\n'"),
        ('文件读／写 Open', 'node -e ' + shlex.quote(file_js)),
        ('网站 TCP Connect', dns_rule['source']['executable']['value'] + ' --silent --show-error --write-out ' + shlex.quote('HTTP=%{http_code}\\n') + ' ' + shlex.quote(url)),
        ('DNS 查询 / DNS query', dns_rule['source']['executable']['value'] + ' -4 --silent --show-error --output /dev/null --write-out ' + shlex.quote('HTTP=%{http_code}\\n') + ' ' + shlex.quote(dns_url)),
    ]
    sections = []
    for title, command in commands:
        task = '请使用 exec 工具逐字执行以下命令并报告真实 stdout 和退出码；AS 弹窗由我回答，不要替换命令：\n' + command
        sections.append('<h2>' + title + '</h2><textarea readonly rows="5" onclick="this.select()">'
                        + html.escape(task) + '</textarea>')
    page = Path('/home/demo/Desktop/AS-DEMO.html')
    page.write_text('<!doctype html><meta charset="utf-8"><title>AS 演示用例</title>'
                    '<style>body{font:17px sans-serif;line-height:1.6}textarea{width:97%;font:15px monospace}</style>'
                    '<h1>复制到 OpenClaw 聊天</h1><p>先完成自己的真实模型 Key／AS 幻影 Key 配置，再逐条发送。'
                    '不会自动请求模型。允许／拒绝由你在 AS 回答。</p><p>实际 Docker 桥网关网站：<a href="'
                    + url + '">' + url + '</a>；仅演示 TCP Connect，不是域名或 Send 验收。</p>'
                    + ''.join(sections))
    english = (ROOT / 'DEMO-TEST-CASES.en.md.template').read_text()
    for key, value in {'{{DEMO_HTTP_ADDRESS}}': f'{address}:{port}', '{{DEMO_HTTP_URL}}': url,
                       '{{DEMO_DNS_NAME}}': dns_domain, '{{DEMO_DNS_URL}}': dns_url}.items():
        english = english.replace(key, value)
    document = Path('/home/demo/Desktop/DEMO-TEST-CASES.en.md')
    document.write_text(english)
    html_document = Path('/home/demo/Desktop/DEMO-TEST-CASES.en.html')
    html_document.write_text('<!doctype html><meta charset="utf-8"><title>AgentShield Demo Test Cases</title>'
        '<style>body{font:16px sans-serif;max-width:1000px;margin:32px auto}pre{white-space:pre-wrap}</style><pre>'
        + html.escape(english) + '</pre>')
    user = pwd.getpwnam('demo')
    os.chown(document, user.pw_uid, user.pw_gid)
    os.chown(html_document, user.pw_uid, user.pw_gid)
    os.chown(page, user.pw_uid, user.pw_gid)


if __name__ == '__main__':
    if sys.argv[1:] == ['--prepare-site']:
        prepare_site()
    elif sys.argv[1:] == ['--port']:
        print(targets()[2])
    else:
        raise SystemExit('usage: demo.py --prepare-site|--port')

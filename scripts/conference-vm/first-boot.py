#!/usr/bin/python3
"""Generate per-clone local credentials; no demo secrets live in the master disk."""
import hashlib
import html
import json
import os
from pathlib import Path
import pwd
import runpy
import secrets
import subprocess
import yaml
import demo as conference_demo


def main():
    state = Path('/var/lib/as-conference')
    if (state / 'initialized').exists():
        return
    demo = pwd.getpwnam('demo')
    # A retry retains credentials already created for this instance.
    identity = state / 'local-identity.json'
    if identity.exists():
        keys = json.loads(identity.read_text())
    else:
        keys = {'as': secrets.token_hex(32), 'openclaw': secrets.token_hex(32),
                'os_password': secrets.token_urlsafe(18)}
        descriptor = os.open(identity, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'w') as output:
            json.dump(keys, output)
    subprocess.run(['/usr/sbin/chpasswd'], input='demo:' + keys['os_password'] + '\n',
                   text=True, check=True)
    path = Path('/opt/agentshield/config/as.yaml')
    config = yaml.safe_load(path.read_text())
    config['middleware']['chat_content'] = True
    config['middleware']['websocket'] = {
        'listen': '127.0.0.1:8765', 'origin': 'http://127.0.0.1:8080',
        'token_sha256': list(hashlib.sha256(keys['as'].encode()).digest()),
    }
    config['group_observation'] = {
        'a2a_port': None, 'otlp_http_port': None, 'gateway_ws_port': 18789,
    }
    path.write_text(yaml.safe_dump(config, sort_keys=False))
    path.chmod(0o644)
    image_info = json.loads(Path('/opt/as-conference/openclaw-image.json').read_text())
    directory = Path('/home/demo/.openclaw')
    directory.mkdir(mode=0o700, exist_ok=True)
    os.chown(directory, image_info['runtime_uid'], image_info['runtime_gid'])
    conference_demo.prepare_file(image_info)
    settings = directory / 'openclaw.json'
    name, provider, defaults = runpy.run_path('/opt/as-conference/openclaw-phantom.py')['client_preset']()
    settings.write_text(json.dumps({'gateway': {
        'mode': 'local', 'auth': {'mode': 'token', 'token': keys['openclaw']},
        'controlUi': {'allowedOrigins': ['http://127.0.0.1:18789']},
    }, 'models': {'providers': {name: provider}}, 'agents': {'defaults': defaults}}, indent=2) + '\n')
    settings.chmod(0o600)
    os.chown(settings, image_info['runtime_uid'], image_info['runtime_gid'])
    image = image_info['local_id']
    env = state / 'openclaw.env'
    env.write_text('OPENCLAW_IMAGE=' + image + '\nOPENCLAW_GATEWAY_TOKEN=' + keys['openclaw'] + '\n')
    env.chmod(0o600)
    welcome = Path('/home/demo/Desktop/START-HERE.html')
    welcome.write_text('''<!doctype html><meta charset="utf-8"><title>AgentShield</title>
<style>body{font:18px sans-serif;max-width:900px;margin:50px auto;line-height:1.7}
textarea{width:95%;font:16px monospace}a{display:inline-block;margin:10px;padding:12px;background:#edf4ff}
</style><h1>AgentShield · OpenClaw</h1>
<p>本机演示环境 / Local conference appliance</p>
<h2>1. AS</h2><p>打开 Dashboard，复制下方本机管理令牌登录。令牌仅属于这台虚拟机。</p>
<p>Open the Dashboard and paste this VM's local management token.</p>
<textarea readonly rows="2" onclick="this.select()">''' + html.escape(keys['as']) + '''</textarea>
<p><a href="http://127.0.0.1:8080" target="_blank">AS Dashboard</a></p>
<h2>2. OpenClaw</h2><p>在 OpenClaw 设置中配置模型服务。演示幻影 Key 时，真实 Key 只填 AS 凭证页面，
在 AS 预置连接绑定已导入凭证并签发幻影 Key，再打开桌面“配置 OpenClaw 幻影 Key”粘贴。
本机首次启动已自动准备真实 CA、容器作用域与 GLM Responses 连接；不需要手写 JSON。模型可在正常设置中修改。</p>
<p>For phantom-key protection, keep the real key in AS and configure OpenClaw with the issued phantom key.</p>
<a href="http://127.0.0.1:18789/#token=''' + keys['openclaw'] + '''" target="_blank">OpenClaw Web UI</a>
<h2>3. Protection / 防护</h2><p>在 AS 规则页面管理 Allow／Deny／Ask；连接 AS 后才有交互问询。
真实模型 Key 只保存在 AS；OpenClaw 只填写签发的幻影 Key。预配完成不等于已经发生模型请求或聊天采集。</p>
<p>Configure controlled TLS and phantom keys through AS before expecting captured LLM conversations.</p>
<p>出厂只询问预置进程、文件、网站连接与 Google DNS 演示目标；其余操作没有这些应用询问规则。系统Driver基线和自保继续。
<a href="AS-DEMO.html" target="_blank">可复制的 OpenClaw 演示任务</a> · <a href="DEMO-TEST-CASES.en.html" target="_blank">English test cases</a></p>
<iframe title="演示任务" src="AS-DEMO.html" style="width:100%;height:850px;border:0"></iframe>
<h2>System / 系统</h2><p>本机管理员为 demo，系统密码在首次启动生成，可自行修改：</p>
<textarea readonly rows="2" onclick="this.select()">''' + html.escape(keys['os_password']) + '''</textarea>
<p>桌面自动登录，sudo 可直接使用。网络默认 NAT，可在虚拟机设置中自行调整。
虚拟机完全关闭后可从快照恢复；不要把已填写个人 Key 或聊天的磁盘再次分发。</p>
<p>The demo user is a local administrator. Redistribute the clean master, not your personalized disk.</p>
<p>组件声明位于 /opt/as-conference；对应源码随发布目录提供。通过桌面菜单正常关机。</p>''')
    welcome.chmod(0o600)
    os.chown(welcome, demo.pw_uid, demo.pw_gid)
    (state / 'initialized').write_text('initialized\n')


if __name__ == '__main__':
    main()

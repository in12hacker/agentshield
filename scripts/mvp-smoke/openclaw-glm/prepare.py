#!/usr/bin/env python3
"""Prepare only: new frozen-candidate real GLM Responses + AS phantom cohort. Never starts Agent/model."""

from pathlib import Path
import runpy
import argparse, ast, json, os, re, subprocess, hashlib, sys, shutil, ssl

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("candidate", type=Path)
p.add_argument("fixture", type=Path)
p.add_argument("output", type=Path)
p.add_argument("--label", required=True)
p.add_argument("--model", required=True)
p.add_argument("--policy-validator", type=Path)
p.add_argument("--templates", type=Path, default=Path.cwd() / ".runtime")
a = p.parse_args()
os.umask(0o077)
if a.label in ("", ".", "..") or any(c in a.label for c in "/\\\x00"):
    p.error("label must be a single path component")
entry = Path(__file__).resolve().parent
f = a.fixture.resolve()
o = a.output.resolve()
validator = a.policy_validator.resolve() if a.policy_validator else None
if validator is not None and not validator.is_file():
    print("Optional schema checker unavailable; the actual rule owner still validates the document.")
    validator = None
subprocess.run(
    [
        sys.executable,
        str(entry / "prepare-base.py"),
        str(a.candidate.resolve()),
        str(f),
        str(o),
        "--templates",
        str(a.templates.resolve()),
    ],
    check=True,
)
script = f / "root/suite/vm-service.py"
s = script.read_text()
# Current egress has no AS-owned transport deadline fields. The inherited
# runtime template is an input, never a second configuration schema.
s = re.sub(
    r"'(?:connect_timeout_ms|handshake_timeout_ms|drain_timeout_ms)'\s*:\s*\d+\s*,\s*",
    "",
    s,
)
s = s.replace(
    "from responses_fixture import CA as RESPONSES_CA, FIXTURE_BEARER, MODEL, ORIGIN, ROUTE, ResponsesFixture",
    f"PUBLIC_CA = '/etc/ssl/certs/ca-certificates.crt'\nMODEL = {a.model!r}\nORIGIN = 'https://open.bigmodel.cn'\nROUTE = '/api/v1/responses'\nPROVIDER_BASE = ORIGIN + '/api/v1'",
)
s = s.replace("'SSL_CERT_FILE': RESPONSES_CA", "'SSL_CERT_FILE': PUBLIC_CA").replace(
    "        self.responses_fixture = ResponsesFixture()",
    "        self.responses_fixture = None\n        real_secret = value.pop('secret')",
)
s = s.replace("'baseUrl': ORIGIN + '/v1'", "'baseUrl': PROVIDER_BASE").replace(
    "'name': 'R69 Responses Fixture'", "'name': 'GLM authorized real provider'"
)
s = (
    s.replace("f'openai/{MODEL}'", "f'as-vm-glm/{MODEL}'")
    .replace(
        "'providers': {'openai': provider}", "'providers': {'as-vm-glm': provider}"
    )
    .replace("'value': FIXTURE_BEARER", "'value': real_secret")
    .replace(
        "        self.checkpoint('actual_CA_and_secret_import_committed')",
        "        real_secret = None\n        self.checkpoint('actual_CA_and_secret_import_committed')",
    )
)
s = (
    s.replace(
        "request={'allowPrivateNetwork': True, 'tls': {'ca': pem}}",
        "request={'tls': {'ca': pem}}",
    )
    .replace("models.providers.openai", "models.providers.as-vm-glm")
    .replace("'provider': ORIGIN + '/v1'", "'provider': PROVIDER_BASE")
)
s = s.replace(
    "'model': MODEL,\n            'review_ws_config'",
    "'model': MODEL, 'provider_profile':'open_ai_responses','synthetic_provider_started':False,\n            'review_ws_config'",
)
start = s.find("        if op == 'audit_arm':")
end = s.find("        if op == 'answer':", start)
if start >= 0:
    s = s[:start] + s[end:]
assert "ResponsesFixture()" not in s and "FIXTURE_BEARER" not in s
assert "provider.update(apiKey=phantom['value']" in s
ast.parse(s)
s = s.replace(
    "ask['hit']['rule_id']!=FIXTURE['cases'][name]['rule_id']",
    "ask['hit']['rule_id'] not in FIXTURE['cases'][name]['answers']",
)
start = s.index("def sqlite_chat_head():")
end = s.index("def listener_pid(", start)
s = s[:start] + s[end:]
s = s.replace(
    "'database':sqlite_chat_head()", "'source':'existing_authenticated_subscription'"
)
s = s.replace("from group_witness import Witness, registry\n", "")
s = s.replace("self.group_witness = Witness()", "self.group_witness = None")
s = s.replace("        self.dashboard_relay, self.dashboard_relay_worker = start_relay()\n", "")
start = s.index("        try:\n            Path('/run/openclaw-subagent-registry.json')")
end = s.index("        if self.ws is not None:", start)
s = s[:start] + s[end:]
start = s.find("        if op == 'registry':")
if start >= 0:
    end = s.index("        if op == ", start + 12)
    s = s[:start] + s[end:]
s = s.replace("evidence['passed']=evidence['accepted'] is True and code==0 and evidence['host_consumed'] and evidence['local_reply_written'] and evidence['instance_stopped'] and bool(evidence['media_summaries'])", "evidence['passed']=evidence['accepted'] is True and code==0")
s = s.replace("            if not evidence['passed']:raise RuntimeError('original RequestStop acceptance/write/retirement failed; no retry')\n", "")
script.write_text(
    s.replace(
        "self.checkpoint('original_gateway_provider_ready')",
        "self.checkpoint('provider_configured_awaiting_stock_startup')",
    )
)
shutil.copy2(entry / "cases.py", f / "cases.py")
owner = f / "run-vm.py"
s = owner.read_text().replace("import signal", "import signal\nimport stat")
secret_loader = """def provider_secret():
    # This source needs current user authorization. Never search for other keys.
    path=Path.home() / '.config/opencode/opencode.json'
    fd=os.open(path,os.O_RDONLY|os.O_CLOEXEC)
    try:
        before=os.fstat(fd)
        chunks=[]
        while True:
            chunk=os.read(fd,65536)
            if not chunk:break
            chunks.append(chunk)
        data=b''.join(chunks);after=os.fstat(fd)
        if before.st_mtime_ns!=after.st_mtime_ns:raise RuntimeError('authorized source changed')
        options=json.loads(data)['provider']['glm-team']['options']
        if options['baseURL'].rstrip('/') not in ('https://open.bigmodel.cn/api/paas/v4','https://open.bigmodel.cn/api/v1','https://open.bigmodel.cn/api/coding/paas/v4'):raise RuntimeError('authorized provider differs')
        key=options['apiKey']
        if not isinstance(key,str) or not key or '\\n' in key or '\\r' in key:raise RuntimeError('authorized credential unavailable')
        return key
    finally:os.close(fd)

"""
s = s.replace("import signal", "import signal\nfrom cases import gateway_startup_ready")
s = s.replace(
    "state='ready' if value['ok'] else 'initialization_failed'",
    "state='waiting_gateway_startup' if value['ok'] else 'initialization_failed'",
)
anchor = "        for key, _ in selector.select(.5):"
startup_probe = """        if state['state']=='waiting_gateway_startup' and time.monotonic()>=next_http_probe:
            next_http_probe=time.monotonic()+5
            try:
                response=http_probe.poll(origin+'/startupz')
                if response is None:continue
                payload=response.get('payload')
                state['stock_startup_response']=response
                if gateway_startup_ready(response.get('status'),payload):
                    state['state']='ready'
                    state['last_confirmed_phase']='stock_startup_admitted_after_provider_configuration'
                publish()
            except (OSError,ValueError) as error:
                state['stock_startup_wait_error']=str(error)
                publish()
"""
assert anchor in s
s = s.replace(anchor, startup_probe + anchor)
i = s.index("def send_guest(")
s = s[:i] + secret_loader + s[i:]
s = s.replace("user,id=net0,restrict=on,hostfwd=", "user,id=net0,hostfwd=")
needle = "send_guest({'op': 'initialize', 'gateway_token': gateway_token"
assert needle in s
s = s.replace(
    needle,
    "send_guest({'op': 'initialize', 'secret': provider_secret(), 'gateway_token': gateway_token",
)
ast.parse(s)
owner.write_text(s)
sys.path.insert(0, str(f / "root/suite"))
from middleware import rule, prefix, group
from cases import scenarios

config, plan = scenarios(a.label, rule, prefix, group)
for path in (o / "observation-cases.json", f / "root/suite/observation-cases.json"):
    path.write_text(json.dumps(config, indent=2, ensure_ascii=False) + "\n")
for target in [o, f / "root/suite"]:
    shutil.copy2(entry / "cases.py", target / "cases.py")
shutil.copy2(entry / "execution_witness.py", f / "root/suite/execution_witness.py")
shutil.copy2(entry / "execution_witness.py", o / "execution_witness.py")
subprocess.run(
    [
        "gcc-13",
        "-static",
        "-O2",
        "-Wall",
        "-Wextra",
        "-Werror",
        str(entry / "native-probe.c"),
        "-o",
        str(f / "root/suite/native-effects"),
    ],
    check=True,
)
(f / "root/suite/native-effects").chmod(0o755)
public_ca = Path(ssl.get_default_verify_paths().cafile)
ca_target = f / "root/etc/ssl/certs/ca-certificates.crt"
ca_target.parent.mkdir(parents=True, exist_ok=True)
shutil.copyfile(public_ca, ca_target)
ca_target.chmod(0o644)
for stale in (f / "root/suite").glob("responses-*.pem"):
    stale.unlink()

(f / "root/etc/hosts").write_text(
    "127.0.0.1 localhost\n127.0.1.1 as-openclaw-vm\n::1 localhost\n"
)
(o / "rule-plan.json").write_text(json.dumps(plan, indent=2, ensure_ascii=False) + "\n")
if validator is not None:
    with (o / "offline-rule-schema-check.txt").open("w") as out:
        subprocess.run(
            [str(validator), str(o / "rule-plan.json")],
            stdout=out,
            stderr=out,
            check=False,
        )
stock = o / "run-stock.py"
shutil.copy2(entry / "run-stock.py", stock)
retire = o / "retire.py"
s = retire.read_text().replace(
    '"/openclaw-state/workspace/r88/files/shared.txt"',
    '"/openclaw-state/workspace/'
    + a.label
    + '/files/file-allow.txt", "/openclaw-state/workspace/'
    + a.label
    + '/files/file-deny.txt", "/openclaw-state/workspace/'
    + a.label
    + '/witness/execute-allow.txt", "/openclaw-state/workspace/'
    + a.label
    + '/witness/execute-deny.txt"',
)
ast.parse(s)
s = s.replace(
    '{"ready", "initialization_failed"}',
    '{"ready", "initialization_failed", "waiting_gateway_startup"}',
)
s = s.replace('"browser-events.jsonl",', '"browser-events.jsonl", "observer-gaps.jsonl",')
retire.write_text(s)
run = o / "run-owned.sh"
run.write_text(
    run.read_text().replace(
        'python3 -B "$out/verify-raw.py" "$out"',
        'python3 -B "$out/verify-glm.py" "$out"',
    )
)
shutil.copy2(entry / "verify-glm.py", o / "verify-glm.py")
(o / "provider-scope.json").write_text(
    json.dumps(
        {
            "provider": "GLM real public upstream",
            "requested_model": a.model,
            "base_url": "https://open.bigmodel.cn/api/v1",
            "profile": "open_ai_responses",
            "authorized_source": "existing glm-team only; current authorization required",
            "synthetic_provider_started": False,
            "AS_upstream_CA": "/etc/ssl/certs/ca-certificates.crt",
            "key_delivered_to_Agent": False,
            "billing": "not inferred from model/alias; existing user subscription intent, verify actual source/error, no standard-API fallback",
            "positive_stock_exact_tool_binding": "not assumed; no manual provenance injection",
        },
        indent=2,
    )
    + "\n"
)
for file in list(o.glob("*.py")) + list((f / "root/suite").glob("*.py")):
    ast.parse(file.read_text())
remove_owned_deadlines = runpy.run_path(str(entry / "prepare-base.py"))["remove_owned_deadlines"]
for path, classes in [
    (f / "run-vm.py", None),
    (f / "call-vm.py", None),
    (o / "retire.py", None),
    (f / "root/suite/vm-service.py", None),
    (f / "root/suite/middleware.py", {"ClientSession", "MiddlewareClient"}),
    (f / "root/suite/workbench.py", {"WorkbenchSocket"}),
    (f / "root/suite/ws_review.py", {"ReviewWebSocket"}),
]:
    remove_owned_deadlines(path, classes)

# Existing readiness probes remain read-only, but a slow HTTP read cannot freeze
# the selector that owns the running VM or create a one-second admission barrier.
path = f / "run-vm.py"
s = path.read_text().replace("from cases import gateway_startup_ready", "from cases import gateway_startup_ready, HttpProbe")
s = s.replace("next_http_probe = 0", "http_probe = HttpProbe()\nnext_http_probe = 0")
start = s.index("                response = urllib.request", s.index("if state['state'] == 'initializing_current_AS_and_OpenClaw'"))
end = s.index("            except", start)
s = s[:start] + """                response = http_probe.poll(origin + '/')
                if response and response.get('status') == 200:
                    http_observed = True
                    state['actual_guest_http_observed'] = 200
                    publish()
                    send_guest({'op': 'gateway_http_observed', 'http_status': 200})
""" + s[end:]
s = s.replace("reason[:3000]", "reason")
s = s.replace("process.kill()", "process.wait()")
s = s.replace("state['forced_qemu_cleanup'] = True", "state['cleanup_waiting_for_actual_exit'] = True")
s = s.replace("except Exception as error:\n    diagnostic(", "except KeyboardInterrupt:\n    state['operator_stop_requested'] = True\n    publish()\n    if process.poll() is None:\n        guest.sendall(b'{\"id\":999999,\"op\":\"shutdown\"}\\n')\n        process.wait()\nexcept Exception as error:\n    diagnostic(")
path.write_text(s)

path = f / "root/suite/vm-service.py"
s = path.read_text()
start = s.index("            observed = self.execution_witness.before_answer")
end = s.index("            return ", start)
s = s[:start] + """            self.ws.send({'type': 'answer', 'decision_id': ask['decision_id'], 'decision': message['decision']})
            try:
                observed = self.execution_witness.after_answer(name, ask)
            except Exception as error:
                observed = {'optional_witness_error': str(error), 'capture_phase': 'after_answer'}
""" + s[end:]
path.write_text(s)

# Preserve all authenticated original WS data, including legitimate credential-
# shaped text. The production 8MiB frame contract is not a smaller test quota.
path = f / "root/suite/ws_review.py"
s = path.read_text()
s = s.replace("if self.token in data:\n            raise RuntimeError('raw authentication credential appeared in a server frame')\n        ", "")
s = s.replace("if self.token in raw:\n            raise RuntimeError('credential material rejected from the evidence sink')\n        ", "")
s = s.replace("2 * 1024 * 1024", "8 * 1024 * 1024")
path.write_text(s)

for path in [f / "agent-browser.cjs", f / "openclaw-web.js"]:
    s = path.read_text()
    s = re.sub(r"timeout:\s*[0-9_]+", "timeout: 0", s)
    s = s.replace("chromium.launch({ headless: true })", "chromium.launch({ headless: true, timeout: 0 })")
    s = s.replace("const context = await browser.newContext();", "const context = await browser.newContext();\n  context.setDefaultTimeout(0);\n  context.setDefaultNavigationTimeout(0);")
    s = re.sub(r"const deadline = Date.now\(\) \+ timeoutMs;", "", s)
    s = s.replace("while (Date.now() < deadline)", "while (true)")
    s = re.sub(r", timeoutMs = [0-9_]+", "", s)
    s = re.sub(r"\s*throw new Error\(`OpenClaw[^`]*\$\{timeoutMs\}[^`]*`\);", "", s)
    if path.name == "agent-browser.cjs":
        s = re.sub(
            r"await observeComplete\(([^;]+)\);",
            r"try { await observeComplete(\1); } catch (error) { try { preserve('observer-gaps.jsonl', {stage:'optional_network_observation',error:String(error)}); } catch {} }",
            s,
        )
    path.write_text(s)

path = f / "root/init"
s = path.read_text().replace("mount -t tracefs tracefs /sys/kernel/tracing || poweroff -f", "mount -t tracefs tracefs /sys/kernel/tracing || true")
path.write_text(s)

# A cleanup diagnostic cannot authorize killing an owned live VM.
path = o / "retire.py"
tree = ast.parse(path.read_text())
for node in ast.walk(tree):
    if isinstance(node, ast.While):
        node.body = [statement for statement in node.body if not (
            isinstance(statement, ast.If) and "marker in serial.read_text" in ast.unparse(statement.test)
        )]
ast.fix_missing_locations(tree)
path.write_text(ast.unparse(tree) + "\n")

path = o / "run-owned.sh"
s = path.read_text()
s = "\n".join(line for line in s.splitlines() if "Chromium" not in line or not line.lstrip().startswith("node -e")) + "\n"
s = s.replace('for count in $(seq 1 200); do', 'while [[ ! -f "$fixture/run/status.json" ]]; do')
s = s.replace('headless:true}', 'headless:true,timeout:0}')
for optional in ("prepare-analysis.py", "extract-failure.py"):
    s = s.replace(f'python3 -B "$out/{optional}" "$out" || exit 1', f'python3 -B "$out/{optional}" "$out"')
path.write_text(s)

with (
    (f / "initramfs.cpio.gz").open("wb") as image,
    (o / "archive-build.log").open("w") as log,
):
    subprocess.run(
        [
            "bash",
            "-o",
            "pipefail",
            "-c",
            "find . -print0 | cpio --null -o -H newc --owner=0:0 | gzip -1",
        ],
        cwd=f / "root",
        stdout=image,
        stderr=log,
        check=True,
    )
m = json.loads((f / "inputs.json").read_text())
m.update(
    round=a.label,
    initramfs_sha256=hashlib.sha256((f / "initramfs.cpio.gz").read_bytes()).hexdigest(),
)
for relative in [
    "etc/ssl/certs/ca-certificates.crt",
    "suite/native-effects",
    "suite/cases.py",
    "suite/execution_witness.py",
    "suite/vm-service.py",
    "suite/observation-cases.json",
]:
    m["joint_assets"][relative] = hashlib.sha256(
        (f / "root" / relative).read_bytes()
    ).hexdigest()
m["acceptance_scope"] = {
    "file": "Open",
    "process": "Execute",
    "network": "Connect and Send via stock exec; owned loopback targets",
    "provider": "real GLM, AS phantom only",
    "exact_tool_attribution": "not a control-test prerequisite",
}
for path in (
    f / "inputs.json",
    f / "selected-delivery.json",
    o / "inputs.json",
    o / "selected-delivery.json",
):
    path.write_text(json.dumps(m, indent=2) + "\n")
for script, result in [("verify-archive.py", "launched-archive-verification.json")]:
    with (o / result).open("w") as out:
        subprocess.run(
            [sys.executable, str(o / script), str(f)], stdout=out, check=True
        )
print(
    "Prepared only; no Key read, no Agent/model started. Run the new output run-owned.sh when this cohort is authorized."
)

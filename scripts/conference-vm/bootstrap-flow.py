#!/usr/bin/python3
"""Provision one clean appliance through the existing AS and Docker interfaces."""
import json
import os
from pathlib import Path
import select
import signal
import socket
import ssl
import subprocess
import sys
import time
import uuid
import demo as conference_demo

STATE = Path('/var/lib/as-conference')
AS = Path('/opt/agentshield')
COMPOSE = ['docker-compose', '--env-file', str(STATE / 'openclaw.env'),
           '--file', '/opt/as-conference/compose.yaml']


class Client:
    def __init__(self, alive):
        self.socket = socket.socket(socket.AF_UNIX)
        while True:
            if not alive():
                raise RuntimeError('AS instance exited')
            try:
                self.socket.connect(str(AS / 'run/middleware.sock'))
                break
            except (FileNotFoundError, ConnectionRefusedError):
                time.sleep(0.1)
        self.reader = self.socket.makefile('rb')

    def command(self, lane, command):
        identity = list(uuid.uuid4().bytes)
        command = dict(command, id=identity)
        self.socket.sendall((json.dumps({'version': 2, lane: command}) + '\n').encode())
        while True:
            raw = self.reader.readline()
            if not raw:
                raise RuntimeError('initial AS command connection ended')
            frame = json.loads(raw)
            if frame.get('version') != 2:
                raise RuntimeError('AS returned a different protocol')
            value = frame.get('private_reply' if lane == 'private' else 'message', {})
            if value.get('id') == identity:
                result = value['result']
                if result['status'] == 'accepted':
                    if (lane == 'message' and command.get('type') == 'control'
                            and command['command'].get('type') == 'request_stop'
                            and result.get('data', {}).get('type') == 'main_stop_requested'):
                        return result
                    continue
                if result['status'] in ('completed', 'failed'):
                    return result
                raise RuntimeError('AS returned an unknown command result status')

    def get(self):
        result = self.command('message', {'type': 'configuration', 'command': {'type': 'get'}})
        if result['status'] != 'completed':
            raise RuntimeError('AS configuration read failed')
        return result['data']['snapshot']

    def stop(self):
        result = self.command('message', {'type': 'control', 'command': {'type': 'request_stop'}})
        if result.get('status') != 'accepted' or result.get('data', {}).get('type') != 'main_stop_requested':
            raise RuntimeError('initial AS did not accept retirement')

    def close(self):
        self.reader.close()
        self.socket.close()


def interrupted(_signal, _frame):
    raise InterruptedError('appliance provisioning stopped')


def wait_ready(pid):
    descriptor = os.pidfd_open(pid)
    waiting = select.poll()
    waiting.register(descriptor, select.POLLIN | select.POLLHUP | select.POLLERR)
    client = None
    try:
        client = Client(lambda: not waiting.poll(0))
        result = client.command('message', {'type': 'control', 'command': {'type': 'get_status'}})
        if (result['status'] != 'completed'
                or result['data']['snapshot']['source_state'] != 'running'):
            raise RuntimeError('AS Driver is not Running')
    finally:
        if client is not None:
            client.close()
        os.close(descriptor)


def main():
    if (STATE / 'bootstrap-complete').exists():
        return
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    process = subprocess.Popen([str(AS / 'bin/as-service')])
    client = None
    succeeded = False
    try:
        client = Client(lambda: process.poll() is None)
        snapshot = client.get()
        if snapshot['document']['flow'] is not None or snapshot['document']['credentials']['bindings']:
            print('AS_CONFERENCE_BOOTSTRAP_PRESERVED_EXISTING_CONFIGURATION', flush=True)
            return
        ca_result = client.command('private', {'command': {'type': 'ensure_ca'}})
        if ca_result['status'] != 'completed':
            raise RuntimeError('AS CA creation failed')
        ca = ca_result['data']['info']
        public = client.command('private', {'command': {'type': 'public_ca', 'ca': ca['id']}})
        if public['status'] != 'completed' or public['data']['info']['id'] != ca['id']:
            raise RuntimeError('AS public CA read failed')
        certificate = Path('/opt/as-conference/as-ca.pem')
        certificate.write_text(ssl.DER_cert_to_PEM_cert(bytes(public['data']['info']['certificate_der'])))
        certificate.chmod(0o644)
        subprocess.run(COMPOSE + ['create', '--no-build'], check=True)
        selected = subprocess.check_output(COMPOSE + ['ps', '--all', '--quiet', 'openclaw'], text=True).splitlines()
        if len(selected) != 1:
            raise RuntimeError('Compose did not return one created container')
        container = subprocess.check_output(['docker', 'inspect', '--format', '{{.Id}}', selected[0]], text=True).strip()
        if len(container) != 64 or any(c not in '0123456789abcdef' for c in container):
            raise RuntimeError('Docker did not return a full container identity')
        instance = {'runtime': 'docker', 'namespace': None, 'id': container}
        gateway = conference_demo.gateway(container)
        rules = client.command('message', {'type': 'rules', 'command': {'type': 'get'}})
        if rules['status'] != 'completed':
            raise RuntimeError('AS rule read failed')
        policy = rules['data']['snapshot']
        merged = policy['document']
        existing = {group['id'] for group in merged['groups']}
        templates = conference_demo.template()
        added = False
        for group in templates['groups']:
            if group['id'] in existing:
                continue
            group['enabled'] = True
            for rule in group['rules']:
                if group['execution']['location'] == 'backend':
                    rule['scope'] = {'kind': 'containers', 'instances': [instance]}
                if rule['target'].get('remote_cidr') == conference_demo.GATEWAY_CIDR:
                    rule['target']['remote_cidr'] = gateway + '/32'
            merged['groups'].append(group)
            added = True
        if added:
            saved_rules = client.command('message', {'type': 'rules', 'command': {
                'type': 'update', 'expected_version': policy['version'], 'document': merged}})
            if saved_rules['status'] != 'completed' or saved_rules['data']['type'] != 'rules_saved':
                raise RuntimeError('AS did not publish the scoped conference rules')
            applied = saved_rules['data'].get('applied')
            if not applied or applied['rule_version'] != saved_rules['data']['receipt']['saved_version']:
                raise RuntimeError('AS did not adopt the actual saved conference rules')
        document = snapshot['document']
        document['flow'] = {
            'enabled': True,
            'scope': {'kind': 'workloads', 'values': [{'kind': 'container', 'instance': instance}]},
            'mode': 'mediate_tls', 'capture_content': True,
            'provider_routes': [{'id': list(uuid.uuid4().bytes), 'origin': 'https://open.bigmodel.cn',
                'method': 'POST', 'path': '/api/v1/responses', 'profile': 'open_ai_responses', 'session': None}],
            'decode_limits': {'max_connection_buffer_bytes': 2097152, 'max_message_bytes': 1048576,
                'max_active_exchanges': 128, 'max_json_depth': 64},
            'event_queue_bytes': 2097152,
            'egress': {'ca': ca['id'], 'max_connections': 128, 'max_streams_per_connection': 128},
        }
        result = client.command('message', {'type': 'configuration', 'command': {
            'type': 'update', 'expected_version': snapshot['version'], 'document': document}})
        facts = result.get('data') if result['status'] == 'completed' else result.get('facts')
        if not facts or facts.get('type') != 'configuration_saved':
            raise RuntimeError('AS did not save the initial Flow configuration')
        stored = client.get()
        if stored['document'] != document or stored['version'] != facts['receipt']['saved_version']:
            raise RuntimeError('AS initial configuration readback differed')
        conference_demo.write_tasks(gateway)
        succeeded = True
    finally:
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        try:
            if process.poll() is None:
                if client is None:
                    client = Client(lambda: process.poll() is None)
                client.stop()
        except Exception:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
            raise
        finally:
            code = process.wait()
            if client is not None:
                client.close()
        if code != 0:
            raise RuntimeError('initial AS retirement failed: ' + str(code))
    if succeeded:
        (STATE / 'bootstrap-complete').write_text('configured through AS management API\n')
        print('AS_CONFERENCE_BOOTSTRAP_COMPLETE', flush=True)


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--wait-ready':
        wait_ready(int(sys.argv[2]))
    elif len(sys.argv) == 1:
        main()
    else:
        raise SystemExit('usage: bootstrap-flow.py [--wait-ready MAINPID]')

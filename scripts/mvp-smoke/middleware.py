#!/usr/bin/env python3
"""Scriptable smoke middleware for the current AS Unix protocol. No MCP or fake backend."""
import argparse
import collections
import contextlib
import copy
import errno
import json
import os
import pathlib
import select
import signal
import socket
import sqlite3
import subprocess
import struct
import sys
import time
import uuid

VERSION = 2
MAX_FRAME = 8 * 1024 * 1024


class SmokeError(RuntimeError):
    pass


def require(condition, reason):
    if not condition:
        raise SmokeError(reason)


def new_id():
    return list(uuid.uuid4().bytes)


def path_value(value, *path):
    for part in path:
        value = value[part]
    return value


class ClientSession:
    """Common scenario-side envelope, correlation and private report handling."""
    def __init__(self, recorder, timeout):
        self.pending = collections.deque()
        self.recorder = recorder or (lambda _: None)
        self.timeout = timeout

    def send(self, message, lane="message"):
        self._send_frame({"version": VERSION, lane: message})

    def _read(self, deadline):
        frame = self._receive_frame(deadline)
        require(frame.get("version") == VERSION, "unexpected transport version")
        lanes = [key for key in ["message", "private_reply", "stream_reply", "business_reply", "diagnostic"] if key in frame]
        require(len(lanes) == 1, "ambiguous backend transport frame")
        require(frame.get("diagnostic", {}).get("type") != "source_unavailable", "native source became unavailable")
        message = frame.get("message", {})
        if message.get("type") == "operation_result":
            require(isinstance(message.get("event"), dict) and "hit" in message and "decision_id" in message,
                    "operation result omitted its current source contract")
        if "private_reply" in frame:
            # Private responses are returned to their original waiter, never event logs.
            self.recorder({"lane": "private_reply", "id": frame["private_reply"].get("id"), "status": frame["private_reply"].get("result", {}).get("status")})
        else:
            self.recorder(frame)
        return frame

    def wait(self, predicate, timeout=None):
        for index, frame in enumerate(self.pending):
            if predicate(frame):
                del self.pending[index]
                return frame
        deadline = time.monotonic() + (self.timeout if timeout is None else timeout)
        while True:
            frame = self._read(deadline)
            if predicate(frame):
                return frame
            require(len(self.pending) < 1024, "unconsumed middleware event capacity exhausted")
            self.pending.append(frame)

    def _wait_reply(self, correlation):
        frame = self.wait(lambda f: (
            f.get("message", {}).get("type") == "reply"
            and f["message"].get("id") == correlation
        ) or (
            f.get("diagnostic", {}).get("type") == "reply_output_unavailable"
            and f["diagnostic"].get("id") == correlation
        ))
        if "diagnostic" in frame:
            raise RuntimeError(
                "reply output unavailable: " + str(frame["diagnostic"].get("code", "unknown"))
                + "; actual command effects are unknown; read the saved and applied state before a new mutation"
            )
        return frame

    def request(self, kind, command, completed=True):
        correlation = new_id()
        self.send({"type": kind, "id": correlation, "command": command})
        frame = self._wait_reply(correlation)
        result = frame["message"]["result"]
        if completed:
            reason = str(result.get("message", ""))
            require(result.get("status") == "completed", "backend command failed: " + str(result.get("code", "unknown")) + ": " + reason)
            return result["data"]
        return result

    def business(self, query):
        correlation = new_id()
        self.send({"id": correlation, "query": query}, "business")
        frame = self.wait(lambda f: f.get("business_reply", {}).get("id") == correlation)
        return frame["business_reply"]["result"]

    def private(self, command):
        correlation = new_id()
        self.send({"id": correlation, "command": command}, "private")
        frame = self.wait(lambda f: f.get("private_reply", {}).get("id") == correlation)
        result = frame["private_reply"]["result"]
        require(result.get("status") == "completed", "private command failed: " + str(result.get("code", "unknown")))
        data = result["data"]
        return data

    def subscribe(self, after=None):
        correlation = new_id()
        self.send({"id": correlation, "command": {"type": "subscribe", "filter": {}, "after": after}}, "stream")
        result = self.wait(lambda f: f.get("stream_reply", {}).get("id") == correlation)["stream_reply"]["result"]
        require(result.get("status") == "subscribed", "chat subscription failed")
        return result["subscription_id"]

    def review(self, available):
        self.send({"type": "review_availability", "available": available})
        # A following ordinary response is a same-stream processing barrier.
        self.request("control", {"type": "get_status"})

    def answer(self, ask, decision):
        require(decision in ask["choices"], "decision was not offered by the original executor")
        self.send({"type": "answer", "decision_id": ask["decision_id"], "decision": decision})


class MiddlewareClient(ClientSession):
    """One Unix JSONL connection bound to the actual OS peer."""
    def __init__(self, endpoint, recorder=None, timeout=15.0, expected_pid=None, expected_uid=0):
        super().__init__(recorder, timeout)
        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            self.socket.settimeout(timeout)
            self.socket.connect(endpoint)
            actual_pid, actual_uid, _ = struct.unpack("3i", self.socket.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            require(actual_uid == expected_uid and (expected_pid is None or actual_pid == expected_pid),
                    "backend OS peer does not match the expected service")
        except BaseException:
            self.socket.close()
            raise
        self.buffer = bytearray()

    def close(self):
        self.socket.close()

    def _send_frame(self, frame):
        payload = json.dumps(frame, separators=(",", ":")).encode() + b"\n"
        require(len(payload) <= MAX_FRAME, "request exceeds transport frame limit")
        self.socket.sendall(payload)

    def _receive_frame(self, deadline):
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            require(remaining > 0, "timed out waiting for backend event")
            ready, _, _ = select.select([self.socket], [], [], remaining)
            require(bool(ready), "timed out waiting for backend event")
            chunk = self.socket.recv(65536)
            require(bool(chunk), "backend connection closed")
            self.buffer.extend(chunk)
            require(len(self.buffer) <= MAX_FRAME, "backend exceeds transport frame limit")
        raw, _, rest = self.buffer.partition(b"\n")
        self.buffer = bytearray(rest)
        return json.loads(raw)


class JsonProcess:
    def __init__(self, argv, stderr):
        self.process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, start_new_session=True)
        self.buffer = bytearray()

    def line(self, timeout=15):
        deadline = time.monotonic() + timeout
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            require(remaining > 0, "fixture did not report its actual result")
            ready, _, _ = select.select([self.process.stdout], [], [], remaining)
            require(bool(ready), "fixture did not report its actual result")
            chunk = os.read(self.process.stdout.fileno(), 65536)
            require(bool(chunk), "fixture ended before its result")
            self.buffer.extend(chunk)
            require(len(self.buffer) <= MAX_FRAME, "fixture output exceeds limit")
        line, _, rest = self.buffer.partition(b"\n")
        self.buffer = bytearray(rest)
        return json.loads(line)

    def send(self, value):
        self.process.stdin.write(json.dumps(value, separators=(",", ":")).encode() + b"\n")
        self.process.stdin.flush()

    def release(self):
        self.process.stdin.write(b"G\n")
        self.process.stdin.flush()

    def wait(self, timeout=15):
        return self.process.wait(timeout=timeout)

    def cleanup(self):
        if self.process.poll() is None:
            try:
                os.killpg(self.process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(self.process.pid, signal.SIGKILL)
                self.process.wait(timeout=3)
            return True
        return False


def prefix(text):
    return {"operator": "prefix", "value": text}


def rule(identifier, operations, target, action, source=None):
    return {"id": identifier, "number": 1, "enabled": True, "scope": {"kind": "all_protected"}, "operations": operations, "source": source or {}, "target": target, "action": action, "log": True}


def group(kind, identifier, execution, rules):
    number = (3 if kind == "file" else 2) if execution["location"] == "driver" else 1
    return {"kind": kind, "id": identifier, "number": number, "enabled": True, "execution": execution, "rules": rules}


class MvpSuite:
    def __init__(self, args):
        self.args = args
        self.output = pathlib.Path(args.output)
        self.output.mkdir(mode=0o700, parents=True, exist_ok=False)
        self.events = open(self.output / "events.jsonl", "w", encoding="utf-8")
        os.chmod(self.output / "events.jsonl", 0o600)
        self.stderr = open(self.output / "fixtures.log", "wb")
        self.service_log = open(self.output / "service.log", "ab")
        self.service = None
        self.service_env = None
        self.client = None
        self.processes = []
        self.report = {"passed": False, "excluded": ["mcp"], "cases": [], "execution": "current native backend through authenticated Unix socket"}
        self.provider_secret = b"mvp-fixture-provider-secret"
        self.fixtures = pathlib.Path(args.fixtures).resolve()
        self.work = self.fixtures / ("mvp-owned-" + uuid.uuid4().hex)
        self.work.mkdir(mode=0o755)
        os.chmod(self.work, 0o755)
        self.data = self.work / "protected-data"
        self.data.mkdir(mode=0o755)

    def record(self, frame):
        self.events.write(json.dumps(frame, separators=(",", ":")) + "\n")
        self.events.flush()

    def case(self, name, action):
        entry = {"name": name, "status": "FAIL"}
        self.report["cases"].append(entry)
        try:
            evidence = action()
            entry.update(status="PASS", evidence=evidence or {})
            print(json.dumps({"case": name, "status": "PASS"}), flush=True)
        except Exception as error:
            entry["reason"] = type(error).__name__ + ": " + str(error)
            if self.service is not None:
                self.service_log.flush()
                data = (self.output / "service.log").read_bytes()[self.service_log_start:][-4096:]
                entry["owned_service_exit"] = self.service.poll()
                entry["owned_service_diagnostic"] = data.decode(errors="replace")
            raise

    def launch(self, *argv):
        process = JsonProcess(list(argv), self.stderr)
        self.processes.append(process)
        return process

    def start_service(self):
        self.service_log.flush()
        self.service_log.seek(0, os.SEEK_END)
        self.service_log_start = self.service_log.tell()
        self.service = subprocess.Popen([self.args.service], stdout=self.service_log, stderr=self.service_log, start_new_session=True, env=self.service_env)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            require(self.service.poll() is None, "service exited before its native initialization")
            try:
                self.client = MiddlewareClient(self.args.socket, self.record, expected_pid=self.service.pid)
                status = self.status()
                require(status["source_state"] == "running" and status["self_protection"] == "enabled", "native protection was not actually adopted")
                return {"service_pid": self.service.pid, "driver_instance": status["instance"]}
            except (ConnectionRefusedError, FileNotFoundError):
                time.sleep(0.05)
        raise SmokeError("service did not expose its authenticated endpoint")

    def stop_service(self):
        if self.client:
            self.client.close()
            self.client = None
        require(self.service is not None, "no owned service exists to retire")
        if self.service.poll() is None:
            self.service.send_signal(signal.SIGTERM)
        return self.wait_service_retirement()

    def request_stop_service(self):
        require(self.service is not None and self.service.poll() is None, "no live owned service accepts a stop request")
        accepted = self.client.request("control", {"type": "request_stop"}, completed=False)
        require(accepted.get("status") == "accepted" and accepted.get("data", {}).get("type") == "main_stop_requested",
                "host did not accept this instance's stop request")
        # This path sends no OS termination signal. Accepted alone is not exit.
        retired = self.wait_service_retirement()
        raw_log = (self.output / "service.log").read_bytes()[self.service_log_start:]
        require(b"AS_MAIN_STOP_REQUESTED" in raw_log, "this host did not consume its stop request")
        self.client.close()
        self.client = None
        evidence = {"accepted": accepted, "retirement": retired, "os_stop_signal_sent": False}
        self.report.setdefault("main_stop_requests", []).append(evidence)
        return evidence

    def wait_service_retirement(self):
        require(self.service is not None, "no owned service exists to observe")
        try:
            code = self.service.wait(timeout=50)
        except subprocess.TimeoutExpired:
            raise SmokeError("service retirement did not complete")
        require(code == 0, "service reported failed actual retirement")
        self.service_log.flush()
        raw_log = (self.output / "service.log").read_bytes()[self.service_log_start:]
        require(b"AS_SCRIPTED_SERVICE_STOPPED" in raw_log, "this service did not report actual resource retirement")
        summaries = [line.decode() for line in raw_log.splitlines() if line.startswith(b"AS_SCRIPTED_MEDIA_SUMMARY ")]
        require(bool(summaries), "missing this service's actual message/log queue observations")
        return {"pid":self.service.pid,"exit":code,"waited":True,"media_summary":summaries[-1]}

    def reconnect(self):
        self.client = MiddlewareClient(self.args.socket, self.record, expected_pid=self.service.pid)
        self.status()

    def status(self):
        return self.client.request("control", {"type": "get_status"})["snapshot"]

    def rules(self, groups, session=None):
        session = self.client if session is None else session
        before = session.request("rules", {"type": "get"})["snapshot"]
        saved = session.request("rules", {"type": "update", "expected_version": before["version"], "document": {"groups": groups}})
        require(saved.get("applied") is not None, "rules saved without actual native adoption")
        require(saved["applied"]["rule_version"] == saved["receipt"]["saved_version"], "adopted policy differs from saved policy")
        return saved

    def file(self, name):
        # Each scenario replaces policy through the real management port before
        # preparing its data. No permanent probe/middleware allow rule is added.
        self.rules([])
        path = self.data / name
        path.write_bytes(b"original")
        path.chmod(0o666)
        return str(path)

    def subject(self, path):
        process = self.launch(str(self.fixtures / "subject"), "open", path, "--wait")
        ready = process.line()
        require(ready.get("phase") == "ready" and ready["pid"] == process.process.pid, "subject identity mismatch")
        return process, ready["pid"]

    def subject_result(self, process, expected_errno):
        value = process.line()
        require(value["errno"] == expected_errno and value["result"] == (0 if expected_errno == 0 else -1), "actual open effect differs from expected rule action")
        require(process.wait() == 0, "subject observer failed")
        return value

    def file_policy(self, path, action, stage="driver"):
        # Protect a data directory for every caller, not one selected PID/binary.
        target = {"path": prefix(str(pathlib.Path(path).parent) + "/")}
        if stage == "driver":
            return [group("file", "mvp-driver-file", {"location": "driver"}, [rule("mvp-file", ["open"], target, action)])]
        return [
            group("file", "mvp-backend-file", {"location": "backend", "stage": stage}, [rule("mvp-file", ["open"], target, action)]),
        ]

    def direct_file(self, allowed, stage="driver"):
        path = self.file("file-" + uuid.uuid4().hex)
        child, _ = self.subject(path)
        self.rules(self.file_policy(path, {"type": "allow"} if allowed else {"type": "deny", "terminate": "none"}, stage))
        child.release()
        expected = 0 if allowed else errno.EPERM
        first = self.subject_result(child, expected)
        # An independent ordinary program must receive the same policy effect.
        program = """import json,os,sys
error=0
try:
    fd=os.open(sys.argv[1],os.O_WRONLY|os.O_TRUNC)
    try: os.write(fd,b'changed')
    finally: os.close(fd)
except OSError as exc: error=exc.errno
print(json.dumps({'pid':os.getpid(),'errno':error,'result':0 if error==0 else -1}),flush=True)
"""
        second = self.launch(sys.executable, "-I", "-B", "-c", program, path)
        second_result = self.subject_result(second, expected)
        return {"native_program": first, "python_program": second_result, "source_unrestricted": True}

    def fixed_denial_messages(self):
        path = self.file("fixed-denial-message")
        original = "  AS已按规则拒绝此操作。请停止重复尝试。\n"
        results = []
        for stage in ["driver", "fast"]:
            child, pid = self.subject(path)
            groups = self.file_policy(path, {"type": "deny", "terminate": "none"}, stage)
            selected = groups[0]["rules"][0]
            selected.update(deny_message=original, log=False)
            saved = self.rules(groups)
            child.release()
            effect = self.subject_result(child, errno.EPERM)
            notice = self.client.wait(lambda f: f.get("message", {}).get("type") == "operation_result"
                and f["message"]["event"]["header"].get("actor_tgid") == pid
                and f["message"].get("hit", {}).get("rule_id") == "mvp-file")["message"]
            require(notice["hit"]["deny_message"] == original, "fixed text changed or was not obtained")
            require(notice["hit"]["log"] is False, "feedback altered logging selection")
            require(notice["hit"]["source"]["version"] == saved["receipt"]["saved_version"], "feedback used another rule version")
            require(notice["result"]["type"] == "receipt" and notice["result"]["outcome"] == "denied", "feedback lost actual denial")
            results.append({"stage": stage, "actual_effect": effect, "original_hit": notice["hit"], "decision_id": notice["decision_id"]})
        self.rules([])
        return {"results": results, "source_unrestricted": True, "log_false_still_reports": True}

    def direct_hit_counts(self):
        path = self.file("direct-hit-counts")
        program = """import json,os,sys
print(json.dumps({'pid':os.getpid()}),flush=True)
for command in sys.stdin:
    if command.strip() == 'stop':
        break
    errors=[]
    for _ in range(int(command)):
        try:
            os.close(os.open(sys.argv[1],os.O_RDONLY))
            errors.append(0)
        except OSError as error:
            errors.append(error.errno)
    print(json.dumps(errors),flush=True)
"""
        child = self.launch(sys.executable, "-I", "-B", "-c", program, path)
        pid = child.line()["pid"]
        require(pid == child.process.pid, "count subject identity mismatch")
        versions = []
        for logged, action, repetitions, expected in [
            (False, {"type": "allow"}, 12, 0),
            (True, {"type": "deny", "terminate": "none"}, 3, errno.EPERM),
        ]:
            groups = self.file_policy(path, action)
            groups[0]["rules"][0]["log"] = logged
            saved = self.rules(groups)["receipt"]["saved_version"]
            versions.append((saved, repetitions))
            child.send(repetitions)
            require(child.line() == [expected] * repetitions, "original open effects differ")
        self.rules([])
        child.send(2)
        require(child.line() == [0, 0], "default allowance failed")
        self.client.request("processes", {"type": "refresh", "target": {"type": "system"}})
        key = self.client.request("processes", {"type": "query", "query": {"type": "resolve_pid", "pid": pid}})["result"]["data"]
        require(key is not None and key["host_tgid"] == pid, "count subject absent from process tree")
        result = self.client.request("processes", {"type": "query", "query": {
            "type": "hits", "key": key, "page": {"limit": 8, "cursor": None}}})["result"]
        records = result["data"]["records"]
        require(len(records) == len(versions), "unexpected rule-hit aggregate")
        for version, expected in versions:
            rows = [item for item in records if item["hit"]["source"]["version"] == version]
            require(len(rows) == 1 and int(rows[0]["count"]) == expected
                    and rows[0]["hit"]["rule_id"] == "mvp-file", "native direct matches missing or duplicated")
        child.process.stdin.write(b"stop\n")
        child.process.stdin.flush()
        require(child.wait() == 0, "count subject did not exit cleanly")
        return {"process": key, "aggregates": records, "incomplete": result["data"]["incomplete"]}

    def file_mutations(self):
        operation_codes = {"mkdir": 5, "unlink": 6, "rmdir": 7, "rename": 9, "exchange": 9,
                           "truncate-path": 18, "truncate-fd": 10, "truncate-open": 10,
                           "map-shared-write": 19}
        root = self.data / "mutations"
        evidence = []
        for phase in ["driver_deny", "ask_allow", "ask_deny"]:
            self.rules([])
            directory = root / phase
            directory.mkdir(parents=True)
            targets = []
            for operation, code in operation_codes.items():
                if phase == "ask_deny" and code not in (10, 18, 19):
                    continue
                path = directory / operation
                destination = directory / (operation + "-destination") if code == 9 else None
                if operation == "rmdir": path.mkdir()
                elif operation != "mkdir": path.write_bytes(b"source")
                if destination: destination.write_bytes(b"destination")
                before = (directory if operation == "mkdir" else path).stat()
                targets.append((operation, code, path, destination, before))
            source_range = {"path": prefix(str(directory) + "/")}
            rename_range = dict(source_range, new_path=prefix(str(directory) + "/"))
            direct = phase == "driver_deny"
            denied = phase != "ask_allow"
            action = {"type": "deny", "terminate": "none"} if direct else {
                "type": "ask", "timeout_ms": 5000, "timeout": {"type": "deny", "terminate": "none"}}
            placement = {"location": "driver"} if direct else {"location": "backend", "stage": "ask"}
            selected = [rule("mvp-file-rename", ["rename"], rename_range, action),
                        rule("mvp-file-mutation", ["create_directory", "unlink", "remove_directory",
                                                   "truncate_path", "truncate_open_file", "map_shared_writable"], source_range, action)]
            selected[1]["number"] = 2
            saved = self.rules([group("file", "mvp-file-mutations", placement, selected)])
            if not direct: self.client.review(True)
            for operation, code, path, destination, before in targets:
                argv = [str(self.fixtures / "subject"), operation, str(path)]
                if destination: argv.append(str(destination))
                child = self.launch(*argv, "--wait")
                ready = child.line()
                require(ready.get("phase") == "ready" and ready["pid"] == child.process.pid, "mutation actor identity mismatch")
                child.release()
                original_ask = None
                if not direct:
                    original_ask = self.client.wait(lambda f: f.get("message", {}).get("type") == "ask")["message"]
                    require(original_ask["event"]["header"]["actor_tgid"] == ready["pid"]
                            and original_ask["event"]["header"]["operation"] == code,
                            "mutation Ask did not retain this original operation")
                    require(child.process.poll() is None, "mutation returned before the user decision")
                    # The VFS still holds directory locks. Do not query that
                    # directory while the original is waiting for its decision.
                    self.client.answer(original_ask, {"type": "deny", "terminate": "none"} if denied else {"type": "allow"})
                effect = child.line()
                require(effect["operation"] == operation and effect["errno"] == (errno.EPERM if denied else 0)
                        and effect["result"] == (-1 if denied else 0), "mutation effect differs from its rule")
                require(child.wait() == 0, "mutation observer did not exit normally")
                result = self.client.wait(lambda f: f.get("message", {}).get("type") == "operation_result"
                    and f["message"]["event"]["header"].get("actor_tgid") == ready["pid"]
                    and f["message"]["event"]["header"].get("operation") == code)["message"]
                require(result["hit"]["rule_id"] == ("mvp-file-rename" if code == 9 else "mvp-file-mutation")
                        and result["hit"]["source"]["version"] == saved["receipt"]["saved_version"],
                        "mutation matched a different rule or generation")
                require(result["result"]["type"] == "receipt"
                        and result["result"]["outcome"] == ("denied" if denied else "allowed"),
                        "mutation lost its actual original receipt")
                if original_ask:
                    require(result["decision_id"] == original_ask["decision_id"]
                            and result["event"] == original_ask["event"]
                            and result["hit"] == original_ask["hit"],
                            "mutation changed the original Ask identity, facts or selected rule")
                payload = result["event"]["payload"]
                require(bytes(payload["path"]) == os.fsencode(path), "mutation source path changed")
                require(payload["device"] == [os.major(before.st_dev), os.minor(before.st_dev)], "mutation target filesystem was not preserved")
                if code == 5:
                    require(payload["inode"] is None and payload["creation_mode"] == 0o755,
                            "mkdir facts inferred an inode or changed the original hook mode")
                else:
                    require(int(payload["inode"]) == before.st_ino, "mutation original inode changed")
                if code == 9:
                    require(bytes(payload["new_path"]) == os.fsencode(destination)
                            and payload["rename_flags"] == (2 if operation == "exchange" else 0),
                            "rename destination or original flags changed")
                    if denied:
                        require(path.read_bytes() == b"source" and destination.read_bytes() == b"destination", "denied rename changed either object")
                    elif operation == "exchange":
                        require(path.read_bytes() == b"destination" and destination.read_bytes() == b"source", "allowed exchange did not swap original objects")
                    else:
                        require(not path.exists() and destination.read_bytes() == b"source", "allowed rename did not move the original name")
                elif code == 18:
                    require(path.read_bytes() == (b"source" if denied else b"so")
                            and payload["open_flags"] is None and payload["map_flags"] is None,
                            "path truncate effect or source facts changed")
                elif code == 10:
                    attributes = payload["truncate_attributes"]
                    require(path.read_bytes() == (b"source" if denied else b"")
                            and int(payload["truncate_size"]) == 0
                            and attributes & 0x2008 == 0x2008
                            and bool(attributes & 0x8000) == (operation == "truncate-open"),
                            "file-object truncate effect, requested size, or actual ATTR bits changed")
                elif code == 19:
                    require(path.read_bytes() == (b"source" if denied else b"Xource")
                            and [int(payload[key]) for key in ("map_requested_prot", "map_effective_prot", "map_flags")] == [3, 3, 1],
                            "shared mapping effect or original hook parameters changed")
                elif operation == "mkdir": require(path.is_dir() if not denied else not path.exists(), "mkdir pre-operation protection failed")
                else: require(path.exists() if denied else not path.exists(), "removal effect is inconsistent")
                evidence.append({"phase": phase, "operation": operation, "actual_effect": effect,
                                 "original_receipt": result, "ask": original_ask})
        self.rules([])
        return {"operations": evidence, "resource_range": str(root), "source_unrestricted": True}

    def ask(self, decision=None, disconnect=False, timeout_only=False):
        path = self.file("ask-" + uuid.uuid4().hex)
        child, pid = self.subject(path)
        action = {"type": "ask", "timeout_ms": 1000 if timeout_only else 5000, "timeout": {"type": "deny", "terminate": "none"}}
        self.rules(self.file_policy(path, action, "ask"))
        self.client.review(True)
        child.release()
        ask = self.client.wait(lambda f: f.get("message", {}).get("type") == "ask")["message"]
        require(ask["event"]["header"]["actor_tgid"] == pid, "Ask refers to another original actor")
        require(child.process.poll() is None, "original operation ended before actual Ask")
        if disconnect:
            self.client.close()
            self.client = None
        elif decision:
            self.client.answer(ask, decision)
        expected = 0 if decision == {"type": "allow"} else errno.EPERM
        result = self.subject_result(child, expected)
        if disconnect:
            self.reconnect()
        if not disconnect:
            receipt = self.client.wait(lambda f: f.get("message", {}).get("type") == "operation_result" and f["message"].get("decision_id") == ask["decision_id"])["message"]
            require(receipt["event"] == ask["event"] and receipt["hit"] == ask["hit"], "completion changed the original Ask facts or selected rule")
            result["operation_result"] = receipt["result"]
        self.client.review(False)
        return result

    def headless(self):
        path = self.file("headless")
        child, pid = self.subject(path)
        groups = self.file_policy(path, {"type": "ask", "timeout_ms": 5000, "timeout": {"type": "deny", "terminate": "none"}}, "ask")
        groups.insert(0, group("file", "mvp-driver-transfer", {"location": "driver"}, [rule("mvp-explicit-transfer", ["open"], {"path": prefix(str(self.data) + "/")}, {"type": "to_user"})]))
        self.rules(groups)
        self.client.close()
        self.client = None
        child.release()
        result = self.subject_result(child, 0)
        self.reconnect()
        return result

    def remembered(self):
        path = self.file("remembered")
        child, _ = self.subject(path)
        groups = self.file_policy(path, {"type": "deny", "terminate": "none"}, "fast")
        groups.append(group("file", "mvp-remembered-file", {"location": "backend", "stage": "remembered"}, [rule("mvp-remembered-allow", ["open"], {"path": prefix(str(self.data) + "/")}, {"type": "allow"})]))
        self.rules(groups)
        child.release()
        return self.subject_result(child, 0)

    def tree(self):
        data = self.client.request("processes", {"type": "query", "query": {"type": "resolve_pid", "pid": self.service.pid}})
        result = data["result"]
        key = result["data"]
        require(key is not None and key["host_tgid"] == self.service.pid, "tree did not resolve actual service")
        detail = self.client.request("processes", {"type": "query", "query": {"type": "get", "key": key}})
        return {"key": key, "detail_kind": detail["result"]["type"]}

    def runtime_and_self(self, self_probe):
        path = self.file("runtime")
        self.rules(self.file_policy(path, {"type": "deny", "terminate": "none"}))
        for feature in ["process", "file", "network"]:
            snapshot = self.client.request("control", {"type": "set_runtime_feature", "feature": feature, "enabled": False})["snapshot"]
            require(snapshot[feature]["effective_state"] == "disabled", "runtime override not actually disabled")
        child, _ = self.subject(path);child.release();self.subject_result(child, 0)
        before = self.client.request("rules", {"type": "get"})["snapshot"]
        self.client.request("rules", {"type": "reload", "expected_version": before["version"]})
        require(self.status()["file"]["effective_state"] == "disabled", "ordinary reload erased temporary override")
        self_probe.release()
        summary = None
        for _ in range(16):
            value = self_probe.line()
            if value.get("phase") == "summary":
                summary = value;break
        require(summary is not None and summary.get("passed") is True, "real self-protection probe failed with ordinary file feature disabled")
        require(self_probe.wait() == 0, "self-protection observer failed")
        for feature in ["process", "file", "network"]:
            snapshot = self.client.request("control", {"type": "resume_runtime_feature", "feature": feature})["snapshot"]
            require(snapshot[feature]["effective_state"] == "enabled", "runtime override did not resume persistent default")
        child, _ = self.subject(path);child.release();self.subject_result(child, errno.EPERM)
        return {"self_protection": summary, "runtime_status": self.status()}

    def termination(self, kind, scope):
        marker = self.file(kind + "-" + scope)
        listener = None
        target = marker
        operation = "open"
        family_kind = kind
        category = "file"
        selector = {"path": prefix(str(self.data) + "/")}
        if kind == "exec":
            category = "process";operation = "execute";target = str(self.fixtures / "rule_kill_probe")
            selector = {"executable": prefix(str(self.fixtures) + "/")}
        elif kind == "send":
            category = "network";operation = "send"
            listener = socket.socket();listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1);listener.bind(("127.0.0.1", 0));listener.listen(1);listener.settimeout(2)
            port = listener.getsockname()[1];target = str(port)
            selector = {"remote_cidr": "127.0.0.0/8", "protocol": 6}
        mode = "hold-parent" if scope == "actor_and_parent" else "release-parent"
        child = self.launch(str(self.fixtures / "rule_kill_probe"), family_kind, target, marker, mode)
        ready = child.line();require(ready.get("phase") == "ready", "family not prepared")
        allowed = scope == "allow"
        action = {"type": "allow"} if allowed else {"type": "deny", "terminate": scope}
        # Retain an already-open read-only witness to observe the actual effect;
        # opening it after a broad deny would itself be an ordinary denied open.
        with open(marker, "rb") as witness:
            self.rules([group(category, "mvp-termination", {"location": "driver"}, [rule("mvp-" + kind + "-" + scope, [operation], selector, action)])])
            child.release()
            received = b""
            if listener:
                connection, _ = listener.accept();connection.settimeout(3)
                with connection:
                    while True:
                        try:
                            part = connection.recv(32)
                        except ConnectionResetError:
                            break
                        if not part:break
                        received += part
                listener.close()
            result = child.line();require(child.wait() == 0, "family observer could not reap originals")
            require(result["actor_signal"] == (9 if scope in ["actor", "actor_and_parent"] else 0), "original actor signal differs")
            require(result["parent_signal"] == (9 if scope == "actor_and_parent" else 0), "original parent signal differs")
            if scope in ["allow", "none"]:
                require(result["actor_exit"] == (0 if allowed else errno.EPERM), "original actor exit differs")
            if kind == "send":
                require(received == (b"changed" if allowed else b""), "actual network bytes differ from rule action")
            else:
                require(witness.read() == (b"changed" if allowed else b"original"), "actual file/exec witness differs from rule action")
            return result

    def start_llm(self, profile="open_ai_chat_completions", tool_path=None, bind="127.0.0.1:0"):
        fixture = str(self.fixtures / "mvp_tls_fixture")
        provider = self.launch(fixture, "--provider")
        provider.line();provider.send({"op": "init", "bind": bind, "expected_secret": self.provider_secret.decode(), "profile": profile, "tool_path": tool_path})
        ready = provider.line();require(ready["event"] == "provider_ready", "TLS provider not initialized")
        # Only this owned service receives a fixture-specific PUBLIC trust root.
        # Never overwrite the host system trust store or place provider keys in env.
        trust = self.work / "provider-ca.pem"
        trust.write_text(ready["ca_pem"])
        self.service_env = {"PATH": "/usr/bin:/bin:/usr/sbin", "SSL_CERT_FILE": str(trust)}
        client = self.launch(fixture, "--client");client.line()
        key_data = self.client.request("processes", {"type": "query", "query": {"type": "resolve_pid", "pid": client.process.pid}})["result"]
        if key_data["data"] is None:
            self.client.request("processes", {"type": "refresh", "target": {"type": "system"}})
            key_data = self.client.request("processes", {"type": "query", "query": {"type": "resolve_pid", "pid": client.process.pid}})["result"]
        key = key_data["data"];require(key is not None, "LLM client's actual process key unavailable")
        ca = self.client.private({"type": "ensure_ca"})["info"]
        secret = self.client.private({"type": "import_secret", "value": self.provider_secret.decode()})["receipt"]
        route_id = new_id();binding_id = new_id()
        snapshot = self.client.request("configuration", {"type": "get"})["snapshot"]
        document = copy.deepcopy(snapshot["document"])
        document["flow"] = {
            "enabled": True, "scope": {"kind": "all_protected"}, "mode": "mediate_tls",
            "provider_routes": [{"id": route_id, "origin": ready["origin"], "method": "POST", "path": ready["path"], "profile": profile, "session": None}],
            "capture_content": True,
            "decode_limits": {"max_connection_buffer_bytes": 2097152, "max_message_bytes": 1048576, "max_active_exchanges": 128, "max_json_depth": 64},
            "event_queue_bytes": 2097152,
            "egress": {"ca": ca["id"], "max_connections": 4, "max_streams_per_connection": 4},
        }
        document["credentials"] = {"bindings": [{"id": binding_id, "secret": secret["secret"], "enabled": True, "subjects": [{"kind": "process", "value": key}], "routes": [route_id], "slot": "authorization_bearer"}]}
        saved = self.client.request("configuration", {"type": "update", "expected_version": snapshot["version"], "document": document}, completed=False)
        require(saved["status"] == "failed" and saved.get("code") == "unavailable" and saved.get("facts", {}).get("type") == "configuration_saved" and "reinitialize_main" in saved["facts"]["receipt"]["required_effects"], "Flow resource change must report saved facts and actual reinitialization need")
        persisted = self.client.request("configuration", {"type": "get"})["snapshot"]
        require(persisted["document"]["flow"] == document["flow"] and persisted["document"]["credentials"] == document["credentials"], "actual saved Flow configuration differs")
        self.request_stop_service();self.start_service()
        current = self.client.request("configuration", {"type": "get"})["snapshot"]
        token = self.client.private({"type": "issue_phantom", "binding": binding_id, "expected_config": current["version"]})
        subscription = self.client.subscribe()
        client.send({"op": "init", "phantom": token["value"], "ca_der": ca["certificate_der"], "endpoint": ready["endpoint"], "origin": ready["origin"], "profile": profile})
        initialized = client.line();require(initialized["event"] == "client_ready", "client did not accept public CA/phantom")
        return provider, client, key, secret, token, subscription

    @staticmethod
    def chat_event(frame, subscription):
        message = frame.get("message", {})
        if message.get("type") != "chat" or message.get("subscription_id") != subscription:
            return None
        return message["envelope"]["event"]

    @staticmethod
    def stop_llm(client, provider):
        client.send({"op": "stop"});require(client.wait() == 0, "client retirement failed")
        provider.send({"op": "stop"});require(provider.wait() == 0, "provider retirement failed")

    def llm(self, bind="127.0.0.1:0"):
        ipv6 = bind.startswith("[")
        provider, client, key, secret, token, subscription = self.start_llm(bind=bind)
        client.send({"op": "request", "seq": 1})
        outcome = client.line();require(outcome["event"] == "client_result" and outcome["http_status"] == 200 and outcome["done_marker"] and outcome["verified_tls"] and outcome["payload_valid"], "real mediated model request failed")
        def chat_event(frame):
            return self.chat_event(frame, subscription)
        for side in ["client_request", "upstream_response"]:
            observed = self.client.wait(lambda f, wanted=side: chat_event(f) is not None and chat_event(f)["type"] == "observation" and chat_event(f)["data"]["source"]["side"] == wanted)
            raw_fact = chat_event(observed)["data"]
            require(raw_fact["source"]["actor"]["process"] == key, "raw observation refers to another process lifetime")
            segments = raw_fact["body"]["segments"]
            require(any(segment.get("kind") == "bytes" and segment.get("data") for segment in segments), "no actual public raw protocol bytes supplied")
            if side == "client_request":
                raw_request = b"".join(bytes(segment["data"]) for segment in segments if segment.get("kind") == "bytes")
                require(json.loads(raw_request)["messages"][0]["content"] == "fixture question", "raw request content differs from the actual sent question")
        seen = self.client.wait(lambda f: chat_event(f) is not None and chat_event(f)["type"] == "finished" and chat_event(f)["data"]["origin"] == "response_output" and chat_event(f)["data"]["role"]["kind"] == "assistant")
        parts = chat_event(seen)["data"]["observed_parts"]
        require(any(part["type"] == "tool_call" and part.get("native_id") is not None for part in parts), "actual model tool call identity was not preserved")
        fact = provider.line();require(fact.get("auth_matches") is True and fact.get("response_prepared") is True, "upstream did not receive real key")
        message = chat_event(seen)["data"]
        require(message["source"]["actor"]["process"] == key and message["state"] == "complete", "finished projection process/state differs from actual client")
        require(any(part["type"] == "text" and part["text"] == "fixture answer" for part in parts), "backend omitted or changed actual model text")
        calls = [part for part in parts if part["type"] == "tool_call"]
        require(len(calls) == 1 and calls[0]["name"] == "fixture_tool" and calls[0]["arguments"] == {"kind":"json","value":{"value":"ok"}}, "backend tool name/arguments differ from actual response")
        require(calls[0]["native_id"]["value"] == {"kind":"json_string","value":fact["tool_call_id"]}, "tool native ID differs from provider's actual generated ID")
        rotated_secret = b"mvp-fixture-rotated-ipv6-secret" if ipv6 else b"mvp-fixture-rotated-secret"
        self.provider_secret = rotated_secret
        changed = self.client.private({"type":"replace_secret","secret":secret["secret"],"expected_revision":secret["revision"],"value":rotated_secret.decode()})
        require(changed["receipt"]["secret"] == secret["secret"] and int(changed["receipt"]["revision"]) > int(secret["revision"]), "rotation did not retain the real secret reference/revision")
        provider.send({"op":"set_secret","expected_secret":rotated_secret.decode()})
        require(provider.line()["event"] == "provider_secret_set", "controlled provider rotation failed")
        client.send({"op":"request","seq":2,"mapped":not ipv6})
        rotated = client.line();require(rotated["http_status"] == 200 and rotated["payload_valid"], "same phantom did not use rotated real secret")
        rotation_fact = provider.line();require(rotation_fact.get("auth_matches") is True, "upstream did not receive rotated key")
        self.client.private({"type": "revoke_phantom", "token": token["token_id"]})
        client.send({"op": "request", "seq": 3})
        denied = client.line();require(denied["http_status"] in [401, 403], "revoked phantom was forwarded")
        provider.send({"op": "stats"})
        stats = provider.line();require(stats["requests"] == 2 and stats["auth_matches"] == 2, "revocation caused another actual upstream request")
        self.stop_llm(client, provider)
        return {"client": outcome, "rotated_client":rotated, "revoked_client": denied, "upstream": fact, "subscription_id": subscription, "actual_process": key}

    def readonly_authorization(self):
        snapshot = self.client.request("configuration", {"type": "get"})["snapshot"]
        policy = self.client.request("rules", {"type": "get"})["snapshot"]
        original_driver = self.status()["instance"]
        document = copy.deepcopy(snapshot["document"])
        document["middleware"]["access"] = "read_only"
        document["middleware"]["chat_content"] = False
        self.service_log.flush()
        log_start = (self.output / "service.log").stat().st_size
        receipt_observed = False
        authorization_close = None
        try:
            result = self.client.request("configuration", {"type": "update", "expected_version": snapshot["version"], "document": document}, completed=False)
            require(result["status"] == "completed", "permission change did not actually persist")
            receipt_observed = True
            denied = self.client.request("rules", {"type": "update", "expected_version": policy["version"], "document": policy["document"]}, completed=False)
            require(denied["status"] == "failed" and denied["code"] == "unauthorized", "old management grant survived revocation")
        except (SmokeError, BrokenPipeError, ConnectionResetError) as error:
            if isinstance(error, SmokeError) and str(error) != "backend connection closed":
                raise
            # Only this explicit permission change may retire an in-progress
            # sensitive write. EOF alone is not proof that the save completed.
            deadline = time.monotonic() + self.client.timeout
            while True:
                require(self.service.poll() is None, "service exited during permission adoption")
                self.service_log.flush()
                lines = (self.output / "service.log").read_bytes()[log_start:].decode(errors="replace").splitlines()
                closed = [line for line in lines if line.startswith("AS_SCRIPTED_SESSION_CLOSED ")]
                if closed:
                    authorization_close = closed[-1]
                    require("code=output_authorization_failed " in authorization_close
                            and ("reason=Unauthenticated" in authorization_close or "reason=Forbidden" in authorization_close),
                            "connection ended without a verified authorization failure")
                    break
                require(time.monotonic() < deadline, "authorization close had no actual service diagnostic")
                time.sleep(0.01)
        self.client.close()
        self.reconnect()
        current = self.client.request("configuration", {"type": "get"})["snapshot"]
        require(current["document"] == document and current["version"] != snapshot["version"], "new authenticated read did not confirm the exact saved permission update")
        require(self.status()["instance"] == original_driver, "permission readback came from a replacement native instance")
        denied = self.client.request("rules", {"type": "update", "expected_version": policy["version"], "document": policy["document"]}, completed=False)
        require(denied["status"] == "failed" and denied["code"] == "unauthorized", "reauthenticated read-only peer obtained rule management")
        self.client.send({"id":new_id(),"command":{"type":"ensure_ca"}}, "private")
        diagnostic = self.client.wait(lambda f: f.get("diagnostic", {}).get("type") == "private_not_authorized")["diagnostic"]
        require(diagnostic["type"] == "private_not_authorized", "read-only connection obtained private management")
        business = self.client.business({"type":"ask_context","decision_id":[0]*16})
        require(business["status"] == "failed" and business["code"] == "unauthorized", "unauthorized business query lost its correlated failure")
        return {"old_management_unavailable":True,"new_access":"read_only","write_receipt_observed":receipt_observed,"authorization_close":authorization_close,"saved_version_readback":current["version"],"private_rejection":diagnostic["type"],"business_rejection":business["code"]}

    def security_log(self):
        path = pathlib.Path(self.args.service).resolve().parent.parent / "log/security.sqlite"
        cases = {case["name"]: case["evidence"] for case in self.report["cases"] if case["status"] == "PASS"}
        logged = cases["file_mutation_driver_deny_and_original_ask_allow"]["operations"]
        unlogged = cases["rule_fixed_denial_text_driver_and_backend"]["results"]
        matched = set()
        with contextlib.closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)) as db:
            for (text,) in db.execute("SELECT record FROM rule_logs ORDER BY id"):
                record = json.loads(text)
                require(record["hit"]["log"] is True, "a rule with logging disabled was persisted")
                for index, item in enumerate(logged):
                    receipt = item["original_receipt"]
                    if record["event"]["header"] != receipt["event"]["header"] or record["hit"] != receipt["hit"]:
                        continue
                    expected = receipt["result"]
                    result = record["result"]["Ok"]
                    outcome = result["driver"] if item["ask"] is None else result["execution"]
                    require(record["event"] == receipt["event"] and record["decision_id"] == receipt["decision_id"]
                            and record["decision"] == {"state": "resolved", **expected["proposal"]}
                            and outcome["outcome"] == expected["outcome"] and outcome["termination"] == expected["termination"],
                            "persisted log differs from the original rule hit, event or operation receipt")
                    require(index not in matched, "same mutation hit persisted twice")
                    matched.add(index)
                for item in unlogged:
                    hit = item["original_hit"]
                    require(record["hit"]["source"] != hit["source"] or record["hit"]["rule_id"] != hit["rule_id"],
                            "log=false rule revision unexpectedly persisted")
        require(len(matched) == len(logged), "an original rule log did not survive service retirement")
        return {"original_rule_logs_read_back": len(matched), "log_disabled_receipts": len(unlogged)}

    def run(self):
        protected = pathlib.Path(self.args.service).resolve().parent.parent / ("mvp-self-" + uuid.uuid4().hex)
        self_probe = self.launch(str(self.fixtures / "self_probe"), str(protected), str(self.work / "self-alias"))
        require(self_probe.line().get("phase") == "ready", "self probe not prepared before activation")
        self.case("native_startup_and_authenticated_status", self.start_service)
        self.case("process_tree_actual_service", self.tree)
        self.case("driver_file_allow", lambda: self.direct_file(True))
        self.case("driver_file_deny", lambda: self.direct_file(False))
        self.case("driver_direct_hit_counts", self.direct_hit_counts)
        self.case("backend_fast_deny", lambda: self.direct_file(False, "fast"))
        self.case("remembered_before_fast", self.remembered)
        self.case("file_mutation_driver_deny_and_original_ask_allow", self.file_mutations)
        self.case("ask_allow", lambda: self.ask({"type": "allow"}))
        self.case("ask_deny", lambda: self.ask({"type": "deny", "terminate": "none"}))
        self.case("ask_timeout_action", lambda: self.ask(timeout_only=True))
        self.case("ask_disconnect_action", lambda: self.ask(disconnect=True))
        self.case("rule_fixed_denial_text_driver_and_backend", self.fixed_denial_messages)
        self.case("headless_skips_ask", self.headless)
        self.case("runtime_overrides_reload_resume_and_self_protection", lambda: self.runtime_and_self(self_probe))
        for kind in ["file", "exec", "send"]:
            for scope in ["allow", "none", "actor", "actor_and_parent"]:
                self.case(kind + "_" + scope, lambda k=kind, s=scope: self.termination(k, s))
        self.rules([])
        self.case("controlled_tls_phantom_raw_chat_and_revoke", self.llm)
        self.case("controlled_tls_ipv6_phantom_raw_chat_and_revoke", lambda: self.llm("[::1]:0"))
        self.case("readonly_policy_revokes_original_manager", self.readonly_authorization)
        self.case("actual_service_shutdown", self.stop_service)
        self.case("persistent_log_database", self.security_log)
        self.report["passed"] = True

    def finish(self):
        forced = []
        for process in reversed(self.processes):
            if process.cleanup():
                forced.append(process.process.pid)
        if forced and self.report["passed"]:
            self.report["passed"] = False
            self.report["retirement_error"] = "fixtures required forced cleanup after claimed success"
        self.report["forced_fixture_cleanup"] = forced
        if self.client:
            self.client.close()
        if self.service and self.service.poll() is None:
            self.service.send_signal(signal.SIGTERM)
            try:self.service.wait(timeout=50)
            except subprocess.TimeoutExpired:
                self.service.kill();self.service.wait(timeout=3)
                self.report["passed"] = False
                self.report["retirement_error"] = "forced service cleanup after timeout"
        self.events.close();self.stderr.close();self.service_log.close()
        output = self.output / "report.json"
        output.write_text(json.dumps(self.report, indent=2) + "\n")
        output.chmod(0o600)


def run_suite(suite):
    try:
        suite.run()
    except Exception as error:
        suite.report["error"] = type(error).__name__ + ": " + str(error)
    finally:
        suite.finish()
    print("AS_MVP_SCRIPTED_SMOKE_PASS" if suite.report["passed"] else "AS_MVP_SCRIPTED_SMOKE_FAIL", flush=True)
    return 0 if suite.report["passed"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--socket", default="/as/run/middleware.sock")
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("status")
    command = sub.add_parser("command")
    command.add_argument("file", help="existing Down JSON body file, or - for stdin")
    run = sub.add_parser("run")
    run.add_argument("--service", required=True)
    run.add_argument("--fixtures", required=True)
    run.add_argument("--output", required=True)
    args = parser.parse_args()
    if args.action == "run":
        return run_suite(MvpSuite(args))
    client = MiddlewareClient(args.socket)
    try:
        if args.action == "status":
            print(json.dumps(client.request("control", {"type": "get_status"}), indent=2))
        else:
            raw = sys.stdin.read() if args.file == "-" else pathlib.Path(args.file).read_text()
            body = json.loads(raw);require("id" in body, "use the current Down body including CommandId")
            client.send(body)
            print(json.dumps(client._wait_reply(body["id"]),indent=2))
    finally:
        client.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())

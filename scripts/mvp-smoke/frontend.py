#!/usr/bin/env python3
"""Repeatable front-end acceptance of a packaged AS guest over its formal WebSocket."""
import argparse
import copy
import errno
import hashlib
import json
import math
import os
from pathlib import Path
import select
import secrets
import socket
import sqlite3
import sys
import time

# The packaged interpreter uses -I. Imports are limited to the owned script directory.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from middleware import ClientSession, MvpSuite, SmokeError, VERSION, group, new_id, prefix, require, rule, run_suite


LISTEN = "127.0.0.1:21854"
ORIGIN = "http://127.0.0.1:21855"
ALLOW = {"type": "allow"}
DENY = {"type": "deny", "terminate": "none"}


def verify_native_return(value, expected_errno, expected_success):
    if expected_errno:
        require(value == -expected_errno, "native return differs from original denied operation")
    elif expected_success is None:
        require(value >= 0, "original open did not return a file descriptor")
    else:
        require(value == expected_success, "native return lost its original status, byte or message count")


class WebSocketClosed(SmokeError):
    pass


class WebSocketSession(ClientSession):
    def __init__(self, process, token, recorder):
        super().__init__(recorder, 15.0)
        self.process = process
        self.ended = False
        require(process.line() == {"transport": "open"}, "WS handshake did not finish")
        self.send({"token": token}, "authenticate")
        authenticated = self._receive_frame(time.monotonic() + self.timeout)
        require(set(authenticated) == {"version", "authenticated"} and authenticated["version"] == VERSION,
                "first WS reply was not the formal authentication reply")
        self.permissions = authenticated["authenticated"]
        self.recorder(authenticated)

    def __enter__(self):
        return self

    def __exit__(self, error_type, _error, _traceback):
        if error_type is None:
            self.close()

    def _send_frame(self, frame):
        self.process.send({"send": frame})
        require(self.process.line() == {"transport": "sent"}, "WS frame was not sent")

    def _receive_frame(self, deadline):
        remaining = deadline - time.monotonic()
        require(remaining > 0, "timed out waiting for WS frame")
        self.process.send({"receive": math.ceil(remaining * 1000)})
        frame = self.process.line(remaining)
        if frame.get("transport") in {"close", "eof"}:
            self.ended = True
            self.recorder(frame)
            raise WebSocketClosed("backend WebSocket ended")
        return frame

    def close(self):
        if not self.ended:
            self.process.send("close")
            closed = self.process.line()
            self.ended = True
            self.recorder(closed)
            require(closed == {"transport": "close", "code": 1000}, "normal WS Close was not confirmed")
        require(self.process.wait() == 0, "WS transport did not actually exit")


def business(session, query):
    reply = session.business(query)
    require(reply["status"] == "completed", "formal WS business query failed")
    return reply["data"]


def processes(session, query):
    return session.request("processes", {"type": "query", "query": query})["result"]


def observed_process(session, pid):
    deadline = time.monotonic() + session.timeout
    while True:
        key = processes(session, {"type": "resolve_pid", "pid": pid})["data"]
        if key is not None:
            require(key["host_tgid"] == pid, "process query changed observed PID")
            return key
        require(time.monotonic() < deadline, "actual original process key missing")
        time.sleep(0.01)


def security_page(session, command, required_ids=(), required_closed=()):
    deadline = time.monotonic() + session.timeout
    while True:
        reply = session.request("security_log", command, completed=False)
        if reply["status"] == "completed":
            page = reply["data"]["page"]
            seen = {tuple(row["record"]["decision_id"]) for row in page["records"]
                    if row["record"].get("decision_id") is not None}
            closed = {bytes(record["path"]) for row in page["records"]
                      if (record := row["record"]).get("kind") == "file_write_close"
                      and record["path"] is not None}
            if all(identity in seen for identity in required_ids) and all(path in closed for path in required_closed):
                return page
        else:
            require(reply["code"] == "busy", "committed security log query failed: " + reply["code"])
        require(time.monotonic() < deadline, "expected committed security log page not observed")
        time.sleep(0.01)


def execution_site(event):
    syscall = event.get("syscall")
    stack = event.get("kernel_stack")
    require(syscall is not None and stack is not None, "original execution site was not captured")
    values = [syscall["instruction_pointer"], syscall["stack_pointer"], *syscall["arguments"],
              *stack["instruction_pointers"]]
    require(len(syscall["arguments"]) == 6 and 0 < len(stack["instruction_pointers"]) <= 32,
            "actual syscall arguments or kernel frames missing")
    require(all(isinstance(value, str) and value.isascii() and value.isdecimal()
                and str(int(value)) == value and int(value) < 2**64 for value in values),
            "execution site lost original u64 representation")
    require(stack["buffer_full"] == (len(stack["instruction_pointers"]) == 32),
            "kernel stack buffer state differs from captured count")
    require(isinstance(syscall["audit_arch"], int) and isinstance(syscall["number"], int),
            "actual syscall ABI or number missing")
    tids = event["header"].get("actor_namespace_tids")
    require(tids and tids[0] == event["header"]["actor_tid"] and len(tids) <= 33,
            "original namespace thread identity is missing")
    user = event.get("user_stack")
    require(user is not None and user["result"] > 0, "BPF user stack was not successfully captured")
    frames = user["snapshot"]["instruction_pointers"]
    require(user["result"] == len(frames) * 8 and frames[0] == syscall["instruction_pointer"]
            and user["snapshot"]["buffer_full"] == (len(frames) == 32),
            "user frames differ from the original syscall or helper result")


def chat(frame):
    message = frame.get("message", {})
    return message["envelope"] if message.get("type") == "chat" else None


def finished_tool(session, tool_id):
    def matches(frame):
        envelope = chat(frame)
        if not envelope or envelope["event"]["type"] != "finished":
            return False
        return any(part["type"] == "tool_call"
                   and (part.get("native_id") or {}).get("value", {}).get("value") == tool_id
                   for part in envelope["event"]["data"]["observed_parts"])
    return chat(session.wait(matches))


class FrontendSuite(MvpSuite):
    def configure(self):
        self.token = secrets.token_hex(32)
        self.session_count = 0
        self.asks = []
        self.ask_actors = []
        self.security_records = []
        self.close_audit_directory = self.fixtures / "close-audit"
        self.close_audit_directory.mkdir()
        self.cursors = []
        self.report["execution"] = "formal WebSocket frontend and native guest"
        config = Path(self.args.service).resolve().parents[1] / "config/as.yaml"
        document = config.read_text()
        require(document.count("websocket: null") == 1, "guest fixture must start with WS disabled")
        websocket = {"listen": LISTEN, "origin": ORIGIN,
                     "token_sha256": list(hashlib.sha256(self.token.encode()).digest())}
        require(document.count("file_close_audit: []") == 1, "guest fixture must start with write-close audit disabled")
        document = document.replace("file_close_audit: []",
                                    "file_close_audit: " + json.dumps([str(self.close_audit_directory)]), 1)
        config.write_text(document.replace("websocket: null", "websocket: " + json.dumps(websocket), 1))

    def connect_ws(self, access="manage_service", chat_content=True, observed=None):
        self.session_count += 1
        identity = self.session_count

        def record(frame):
            self.record({"ws_session": identity, "frame": frame})
            if observed is not None and (envelope := chat(frame)) is not None:
                observed.append(envelope)

        process = self.launch(str(self.fixtures / "mvp_ws_client"), LISTEN, ORIGIN)
        session = WebSocketSession(process, self.token, record)
        require(session.permissions == {"access": access, "chat_content": chat_content},
                "actual WS authorization differs")
        return session

    def protection(self):
        path = self.file("ws-remembered")
        with self.connect_ws() as ws:
            version = ws.request("configuration", {"type": "get"})["snapshot"]["version"]
            rule = self.file_policy(path, DENY, "remembered")
            saved = self.rules(rule, ws)
            snapshot = ws.request("rules", {"type": "get_rule", "rule_id": "mvp-file"})["snapshot"]
            require(snapshot["version"] == saved["receipt"]["saved_version"]
                    and snapshot["group_id"] == rule[0]["id"], "rule readback differs from adopted version")
            for expected in (errno.EPERM, 0, errno.EPERM):
                if expected == 0:
                    state = ws.request("control", {"type": "set_runtime_feature", "feature": "file", "enabled": False})
                    require(state["snapshot"]["file"]["effective_state"] == "disabled", "temporary state not adopted")
                child, _ = self.subject(path)
                child.release()
                self.subject_result(child, expected)
                if expected == 0:
                    state = ws.request("control", {"type": "resume_runtime_feature", "feature": "file"})
                    require(state["snapshot"]["file"]["effective_state"] == "enabled", "file default not restored")
            current = ws.request("configuration", {"type": "get"})["snapshot"]
            require(current["version"] == version, "temporary override changed saved configuration")
            document = copy.deepcopy(current["document"])
            protection = document["driver"]["self_protection"]
            protection["log"] = not protection["log"]
            updated = ws.request("configuration", {"type": "update", "expected_version": version, "document": document})
            actual = ws.request("configuration", {"type": "get"})["snapshot"]
            require(updated["applied"] is not None and actual["document"] == document
                    and actual["version"] == updated["receipt"]["saved_version"], "WS config save/adopt/readback mismatch")
            ca = ws.private({"type": "ensure_ca"})["info"]
            require(ws.private({"type": "public_ca", "ca": ca["id"]})["info"] == ca, "WS private CA mismatch")
            self.rules([], ws)
        return {"actual_open_errno": [errno.EPERM, 0, errno.EPERM], "saved_rule": snapshot["version"],
                "saved_configuration": actual["version"], "public_ca_roundtrip": True}

    def original_question(self, ws, child, pid, kind, operation):
        ask = ws.wait(lambda f: f.get("message", {}).get("type") == "ask")["message"]
        header = ask["event"]["header"]
        require(header["actor_tgid"] == pid and header["kind"] == kind
                and header["operation"] == operation and child.process.poll() is None,
                "Ask does not identify this waiting original operation")
        execution_site(ask["event"])
        task = Path(f"/proc/{pid}/task/{header['actor_tid']}")
        nspid = next(line.split()[1:] for line in (task / "status").read_text().splitlines()
                     if line.startswith("NSpid:"))
        require(header["actor_namespace_tids"] == list(map(int, nspid)),
                "namespace TIDs differ from the actual waiting thread")
        mount_inode = os.readlink(task / "ns/mnt").split("[", 1)[1].rstrip("]")
        require(ask["event"]["process"]["mount_namespace_inode"] == mount_inode,
                "mount namespace differs from the actual waiting thread")
        require(int((task / "syscall").read_text().split()[0]) == ask["event"]["syscall"]["number"]
                and "[as_wait]" in (task / "stack").read_text(),
                "original syscall is not waiting in the actual AS module")
        require(ask["choices"] == [ALLOW, DENY], "formal Ask choices changed")
        return ask

    def original_receipt(self, ws, ask, decision, expected_errno, key, expected_return=None):
        receipt = ws.wait(lambda f: f.get("message", {}).get("type") == "operation_result"
                          and f["message"]["decision_id"] == ask["decision_id"])["message"]
        result = receipt["result"]
        require(receipt["event"] == ask["event"] and receipt["hit"] == ask["hit"]
                and result["proposal"] == {"decision": decision, "reason": "user"}
                and result["outcome"] == ("allowed" if expected_errno == 0 else "denied"),
                "formal WS result lost original action or identity")
        self.asks.append((ask["decision_id"], expected_errno, ask["event"], expected_return))
        self.ask_actors.append((key, ask["hit"]))

    def family_question(self, ws, family, decision, expected_errno, action):
        marker = self.file("ws-ask-" + family)
        datagram = family == "sendmmsg"
        with socket.socket(type=socket.SOCK_DGRAM if datagram else socket.SOCK_STREAM) as listener:
            connection = None
            try:
                if family == "exec":
                    kind, operation, code = "process", "execute", 1
                    target = str(self.fixtures / "rule_kill_probe")
                    selector = {"executable": prefix(str(self.fixtures) + "/")}
                else:
                    kind, operation, code = "network", ("connect" if family == "connect" else "send"), (1 if family == "connect" else 2)
                    listener.bind(("127.0.0.1", 0))
                    if not datagram:
                        listener.listen(1)
                    listener.settimeout(3)
                    port = listener.getsockname()[1]
                    target = str(port)
                    selector = {"remote_cidr": "127.0.0.1/32", "protocol": 17 if datagram else 6,
                                "remote_port": {"first": port, "last": port}}
                child = self.launch(str(self.fixtures / "rule_kill_probe"), family,
                                    target, marker, "release-parent")
                ready = child.line()
                require(ready.get("phase") == "ready", "native family subject not ready")
                key = observed_process(ws, ready["actor"])
                self.rules([group(kind, "mvp-ws-ask", {"location": "backend", "stage": "ask"},
                                  [rule("mvp-" + kind + "-" + operation, [operation], selector, action)])], ws)
                with open(marker, "rb") as witness:
                    child.release()
                    ask = self.original_question(ws, child, ready["actor"], kind, code)
                    if family == "exec":
                        require(witness.read() == b"original", "target image executed before the answer")
                    elif family == "connect":
                        require(not select.select([listener], [], [], 0)[0], "target connected before the answer")
                    elif datagram:
                        require(not select.select([listener], [], [], 0)[0], "datagram sent before the answer")
                    else:
                        connection, _ = listener.accept()
                        connection.settimeout(3)
                        require(not select.select([connection], [], [], 0)[0], "target sent or closed before the answer")
                    ws.answer(ask, decision)
                    actual = child.line()
                    require(child.wait() == 0 and actual["actor_signal"] == 0
                            and actual["parent_signal"] == 0 and actual["actor_exit"] == expected_errno,
                            "original native operation differs from selected action")
                    if family == "exec":
                        witness.seek(0)
                        require(witness.read() == (b"changed" if expected_errno == 0 else b"original"),
                                "target image effect differs from selected action")
                    elif family == "connect":
                        if expected_errno == 0:
                            connection, _ = listener.accept()
                        else:
                            require(not select.select([listener], [], [], 0)[0], "denied target connection was accepted")
                    elif datagram:
                        if expected_errno == 0:
                            received, _ = listener.recvfrom(32)
                            require(received == b"changed", "actual datagram differs from selected action")
                        else:
                            require(not select.select([listener], [], [], 0)[0], "denied datagram was received")
                    else:
                        received = b""
                        while True:
                            block = connection.recv(32)
                            if not block:
                                break
                            received += block
                        require(received == (b"changed" if expected_errno == 0 else b""),
                                "actual network bytes differ from selected action")
                    expected_return = 0 if family in ("exec", "connect") else 1 if datagram else 7
                    self.original_receipt(ws, ask, decision, expected_errno, key, expected_return)
            finally:
                if connection is not None:
                    connection.close()
            self.rules([], ws)

    def questions(self):
        path = self.file("ws-ask")
        action = {"type": "ask", "timeout_ms": 5000, "timeout": DENY}
        with self.connect_ws() as ws:
            self.rules(self.file_policy(path, action, "ask"), ws)
            ws.review(True)
            for decision, expected in ((ALLOW, 0), (DENY, errno.EPERM)):
                child, pid = self.subject(path)
                key = observed_process(ws, pid)
                child.release()
                ask = self.original_question(ws, child, pid, "file", 1)
                ws.answer(ask, decision)
                actual = self.subject_result(child, expected)
                self.original_receipt(ws, ask, decision, actual["errno"], key)
            self.rules([], ws)
            for family in ("exec", "connect", "send", "sendmsg", "send-write", "send-writev", "sendmmsg"):
                for decision, expected in ((ALLOW, 0), (DENY, errno.EPERM)):
                    self.family_question(ws, family, decision, expected, action)
        return {"original_questions": [{"decision_id": identity, "errno": error,
                                       "kind": event["header"]["kind"], "operation": event["header"]["operation"],
                                       "syscall": event["syscall"], "kernel_stack": event["kernel_stack"],
                                       "user_stack": event["user_stack"],
                                       "namespace_tids": event["header"]["actor_namespace_tids"],
                                       "mount_namespace_inode": event["process"]["mount_namespace_inode"]}
                                      for identity, error, event, _expected_return in self.asks]}

    def runtime_audit(self):
        with self.connect_ws() as ws:
            tree = processes(ws, {"type": "status"})["data"]
            require(tree["nodes"] > 0, "running tree has no observed processes")
            for key, hit in self.ask_actors:
                details = processes(ws, {"type": "get", "key": key})["data"]
                require(details["record"]["key"] == key, "audit read changed the original ProcessKey")
                ancestry = processes(ws, {"type": "ancestors", "key": key, "limit": 4})
                require(ancestry["type"] == "ancestors", "ancestor query returned another kind")
                hits = processes(ws, {"type": "hits", "key": key, "page": {"limit": 8}})["data"]["records"]
                require(any(item["hit"] == hit and int(item["count"]) >= 1 for item in hits),
                        "tree did not report the actual original rule hit")

            listing = {"type": "list", "filter": {}, "page": {"limit": 1}}
            first = processes(ws, listing)["data"]
            require(len(first["records"]) == 1 and first["next_cursor"] is not None, "first tree page has no cursor")
            listing["page"]["cursor"] = first["next_cursor"]
            second = processes(ws, listing)["data"]
            require(len(second["records"]) == 1 and first["records"][0]["key"] != second["records"][0]["key"],
                    "tree cursor repeated an original process")
            listing["filter"]["workload"] = {"kind": "container", "instance": {
                "runtime": "docker", "namespace": None, "id": "0" * 64}}
            mismatch = ws.request("processes", {"type": "query", "query": listing}, completed=False)
            require(mismatch["status"] == "failed" and mismatch["code"] == "cursor_expired", "tree cursor crossed query scope")
            listing["page"].pop("cursor")
            require(processes(ws, listing)["data"]["records"] == [], "unobserved container identity matched")
            refused = ws.request("processes", {"type": "refresh", "target": {"type": "system"}}, completed=False)
            require(refused["status"] == "failed" and refused["code"] == "unsupported", "WS exposed process refresh")

            security_page(ws, {"limit": 64}, [tuple(identity) for identity, _, _, _ in self.asks])
            head = security_page(ws, {"limit": 1})
            require(len(head["records"]) == 1 and head["next_before"] == head["records"][0]["id"],
                    "security log page lost its original row boundary")
            tail = security_page(ws, {"before": head["next_before"], "limit": 64})
            self.security_records = head["records"] + tail["records"]
            ids = [int(row["id"]) for row in self.security_records]
            require(ids == sorted(set(ids), reverse=True), "security log cursor repeated or reordered records")
            for identity, _, _, _ in self.asks:
                require(any(row["record"].get("decision_id") == identity for row in self.security_records),
                        "actual logged Ask is absent from committed security log")
            invalid = ws.request("security_log", {"limit": 0}, completed=False)
            require(invalid["status"] == "failed" and invalid["code"] == "invalid", "invalid log page accepted")
        return {"tree_instance": tree["instance"], "observed_hit_actors": len(self.ask_actors),
                "security_log_ids": ids}

    def write_close_audit(self):
        directory = self.close_audit_directory
        target = directory / "write-open-without-write"
        barrier = directory / "other-write-close"
        readonly = directory / "read-only"
        nested = directory / "nested"
        nested.mkdir()
        nested_file = nested / "unmarked"
        # Create files through read-only opens, which do not produce write-close notifications.
        for path in (target, barrier, readonly, nested_file):
            os.close(os.open(path, os.O_RDONLY | os.O_CREAT, 0o600))
        target_stat = target.stat()
        os.close(os.open(nested_file, os.O_RDWR))
        os.close(os.open(readonly, os.O_RDONLY))
        fd = os.open(target, os.O_RDWR)
        duplicate = os.dup(fd)
        os.close(fd)
        try:
            os.close(os.open(barrier, os.O_RDWR))
            with self.connect_ws() as ws:
                def records(*paths):
                    page = security_page(ws, {"limit": 64}, required_closed=tuple(os.fsencode(p) for p in paths))
                    return [row for row in page["records"] if row["record"].get("kind") == "file_write_close"]

                before = records(barrier)
                require([bytes(row["record"]["path"]) for row in before] == [os.fsencode(barrier)],
                        "first duplicate close, read-only close, or unmarked subdirectory was logged")
                os.close(duplicate)
                duplicate = None
                after = records(target, barrier)
                require([bytes(row["record"]["path"]) for row in after] ==
                        [os.fsencode(target), os.fsencode(barrier)], "write-close source lost or duplicated an original")
                actual = after[0]["record"]
                require(actual == {"kind": "file_write_close", "reporter_tid": os.getpid(),
                                   "path": list(os.fsencode(target)), "path_error": None,
                                   "device_major": os.major(target_stat.st_dev),
                                   "device_minor": os.minor(target_stat.st_dev),
                                   "inode": str(target_stat.st_ino)}, "write-close facts differ from the original object")
                self.security_records.extend(after)
        finally:
            if duplicate is not None:
                os.close(duplicate)
        require(target.read_bytes() == b"", "fixture performed an unexpected write")
        return {"write_close_records": [row["id"] for row in after], "actual": actual}

    def chat_workbench(self):
        provider, actor, _, _, _, subscription = self.start_llm()
        unsubscribe = new_id()
        self.client.send({"id": unsubscribe, "command": {"type": "unsubscribe", "subscription_id": subscription}}, "stream")
        result = self.client.wait(lambda f: f.get("stream_reply", {}).get("id") == unsubscribe)["stream_reply"]["result"]
        require(result["status"] == "unsubscribed", "Unix fixture subscription remained open")

        def model_request(sequence):
            actor.send({"op": "request", "seq": sequence})
            outcome, upstream = actor.line(), provider.line()
            require(outcome.get("http_status") == 200 and outcome.get("verified_tls")
                    and outcome.get("payload_valid") and upstream.get("auth_matches"), "real mediated request failed")
            return upstream["tool_call_id"]

        first_live, resumed = [], []
        with self.connect_ws(observed=first_live) as ws:
            ws.subscribe()
            first = finished_tool(ws, model_request(1))
        second_tool = model_request(2)
        with self.connect_ws(observed=resumed) as ws:
            ws.subscribe(after=first_live[-1]["cursor"])
            second = finished_tool(ws, second_tool)["event"]["data"]
            exchange = second["id"]["exchange"]
            newest = business(ws, {"type": "chat_exchanges", "session": None, "after": None, "limit": 1})
            older = business(ws, {"type": "chat_exchanges", "session": None, "after": newest["next_after"], "limit": 1})
            require(newest["exchanges"] == [exchange] and newest["next_after"] == exchange
                    and older["exchanges"] == [first["event"]["data"]["id"]["exchange"]]
                    and older["next_after"] is None, "exchange paging changed original identities")
            query = {"type": "chat_messages", "scope": {"kind": "model_http", "exchange": exchange}, "after": None, "limit": 1}
            page = business(ws, query)
            require(len(page["messages"]) == 1 and page["next_after"] == {"kind": "model_http", "message": page["messages"][0]["message"]["id"]},
                    "message page did not return its original cursor")
            rest = business(ws, {**query, "after": page["next_after"], "limit": 64})
            versions = [entry["message"] for entry in page["messages"] + rest["messages"] if entry["kind"] == "model_http" and entry["message"]["id"] == second["id"]]
            require(len(versions) == 1 and rest["next_after"] is None, "message paging duplicated or lost the original")
            historical = versions[0]

            def same_revision(envelope):
                event = envelope["event"]
                return (event["type"] in ("started", "finished") and event["data"]["id"] == historical["id"]
                        and event["data"]["revision"] == historical["revision"])

            original = next((frame for frame in resumed if same_revision(frame)), None)
            if original is None:
                original = chat(ws.wait(lambda f: (item := chat(f)) is not None and same_revision(item)))
            require(original["event"]["data"] == historical, "same-revision history differs from live original")
            tool = next(part for part in second["observed_parts"] if part["type"] == "tool_call")
            anchor = {"kind": "part", "value": {"message": second["id"], "part_id": tool["id"]}}
            view = business(ws, {"type": "activity_context", "anchor": anchor})["view"]
            require(view["anchor"] == anchor and {"at": anchor, "reason": "no_explicit_relation"} in view["gaps"],
                    "activity query changed its anchor or execution gap")
            require(any(relation["kind"] == "same_exchange"
                        and relation["from"]["value"] in second["record_refs"]
                        and relation["to"] == {"kind": "message", "value": second["id"]}
                        for relation in view["relations"]), "activity omitted original protocol evidence")
            require(all(relation["kind"] != "tool_execution" for relation in view["relations"]),
                    "activity asserted an execution link without a source")
        self.stop_llm(actor, provider)
        self.cursors = [frame["cursor"] for frame in first_live + resumed]
        return {"exchanges": 2, "live_frames": len(first_live), "resumed_frames": len(resumed),
                "history_message": historical["id"], "history_revision": historical["revision"], "activity_anchor": anchor}

    def tool_attribution(self):
        path = self.file("tool-attribution")
        provider, actor, process, _, _, _ = self.start_llm("open_ai_responses", tool_path=path)
        with self.connect_ws() as ws:
            ws.subscribe()
            actor.send({"op": "request", "seq": 1})
            response, upstream = actor.line(), provider.line()
            require(response.get("http_status") == 200 and upstream.get("auth_matches"),
                    "real tool response failed")
            message = finished_tool(ws, upstream["tool_call_id"])["event"]["data"]
            part = next(part for part in message["observed_parts"] if part["type"] == "tool_call")
            expected = {"message": message["id"], "part_id": part["id"]}

            self.rules(self.file_policy(path, {"type": "ask", "timeout_ms": 5000, "timeout": DENY}, "ask"), ws)
            ws.review(True)
            actor.send({"op": "execute_call"})
            ask = ws.wait(lambda frame: frame.get("message", {}).get("type") == "ask")["message"]
            header = ask["event"]["header"]
            require(header["actor_tgid"] == actor.process.pid and ask["event"]["host_boot_id"] == process["boot_id"],
                    "original Open event lacks its actor or boot scope")
            event = {field: header[field] for field in ("source_instance", "sequence_domain", "sequence")}
            query = {"type": "tool_attribution", "event": event, "after": None, "limit": 1,
                     "pattern": {"actor": "same_thread", "lookback_ns": "30000000000",
                                 "target": {"type": "file_path", "pointer": "/path"}}}
            page = business(ws, query)["view"]
            require(page["required_connection_process"] == process and page["required_connection_thread"]["tid"] == header["actor_tid"],
                    "attribution used a different connection process or thread")
            candidates = page["candidates"]
            require(page["next_after"] is None and len(candidates) == 1
                    and candidates[0]["call"] == expected and candidates[0]["target_comparison"] == "equal",
                    "original model tool call was not matched to the Open event")
            context = business(ws, {"type": "ask_context", "decision_id": ask["decision_id"],
                                    "attribution": {key: query[key] for key in ("pattern", "after", "limit")}})["context"]
            require(context["trigger_call"] is None and context["window"] is None,
                    "pattern matches became confirmed execution evidence")
            require(context["attribution"] == page and len(context["candidate_windows"]) == 1,
                    "Ask context did not reuse the same attribution result")
            window = context["candidate_windows"][0]
            require(window["exchange"] == message["id"]["exchange"]
                    and message["id"] in window["message_refs"] and len(window["message_refs"]) <= 20,
                    "Ask candidate context omitted the original tool message")
            originals = [record["value"] for record in context["view"]["source_records"]
                         if record["kind"] == "message"]
            require(sum(record["id"] == message["id"] for record in originals) == 1
                    and next(record for record in originals if record["id"] == message["id"]) == message,
                    "Ask candidate context duplicated or changed the original message")
            ws.answer(ask, ALLOW)
            effect = actor.line()
            require(effect.get("event") == "tool_executed" and effect.get("errno") == 0,
                    "allowed original tool open failed")
            result = ws.wait(lambda frame: frame.get("message", {}).get("type") == "operation_result"
                             and frame["message"]["decision_id"] == ask["decision_id"])["message"]
            require(result["event"] == ask["event"] and result["result"]["outcome"] == "allowed",
                    "original completion changed while evaluating tool candidates")
        self.rules([])
        self.stop_llm(actor, provider)
        return {"event": event, "candidates": candidates, "candidate_windows": context["candidate_windows"],
                "confirmed_relation": None, "actual_open_errno": effect["errno"]}

    def readonly(self):
        with self.connect_ws() as manager:
            current = manager.request("configuration", {"type": "get"})["snapshot"]
            document = copy.deepcopy(current["document"])
            document["middleware"].update(access="read_only", chat_content=False)
            manager.send({"type": "configuration", "id": new_id(), "command": {
                "type": "update", "expected_version": current["version"], "document": document}})
            try:
                manager._read(time.monotonic() + manager.timeout)
            except WebSocketClosed:
                pass
            else:
                raise SmokeError("revoked manager WS still delivered a reply")
        with self.connect_ws("read_only", False) as reader:
            actual = reader.request("configuration", {"type": "get"})["snapshot"]
            require(actual["document"] == document, "fresh read-only grant did not read actual saved policy")
            rules = reader.request("rules", {"type": "get"})["snapshot"]
            denied = reader.request("rules", {"type": "reload", "expected_version": rules["version"]}, completed=False)
            require(denied["status"] == "failed" and denied["code"] == "unauthorized", "ReadOnly rule mutation was not rejected")
            require(business(reader, {"type": "protection_overview"})["driver"]["status"] == "completed",
                    "ReadOnly protection overview failed")
            denied_chat = reader.business({"type": "chat_exchanges", "session": None, "after": None, "limit": 1})
            require(denied_chat["status"] == "failed" and denied_chat["code"] == "unauthorized", "content grant was bypassed")
            require(processes(reader, {"type": "status"})["type"] == "status", "ReadOnly tree query failed")
            denied_log = reader.request("security_log", {"limit": 1}, completed=False)
            require(denied_log["status"] == "failed" and denied_log["code"] == "unauthorized",
                    "ReadOnly received a full security record")
        return {"new_access": "read_only", "saved_version": actual["version"], "old_connection_ended": True}

    def audit_database(self):
        path = Path(self.args.service).resolve().parents[1] / "data/middleware/business.sqlite"
        request_returns = []
        with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as database:
            for identity, expected_errno, event, expected_return in self.asks:
                execution_site(event)
                original = database.execute("SELECT event FROM questions WHERE decision_id=?", (bytes(identity),)).fetchone()
                require(original is not None, "original Ask missing from business database")
                stored = database.execute(
                    "SELECT audit_arch,number,instruction_pointer,stack_pointer,arg0,arg1,arg2,arg3,arg4,arg5 "
                    "FROM event_syscalls WHERE event=?", original).fetchone()
                source = event["syscall"]
                expected = (source["audit_arch"], source["number"],
                            *[int(value).to_bytes(8, "big") for value in
                              [source["instruction_pointer"], source["stack_pointer"], *source["arguments"]]])
                require(stored == expected, "SQLite syscall values differ from original WS Ask")
                for kind, source, result in (("kernel", event["kernel_stack"], None),
                                             ("user", event["user_stack"]["snapshot"], event["user_stack"]["result"])):
                    stack = database.execute("SELECT buffer_full,result FROM event_stacks WHERE event=? AND kind=?", (*original, kind)).fetchone()
                    frames = database.execute("SELECT f.instruction_pointer FROM event_stack_frames f JOIN event_stacks s ON s.id=f.stack WHERE s.event=? AND s.kind=? ORDER BY f.ordinal", (*original, kind)).fetchall()
                    require(stack == (int(source["buffer_full"]), result)
                            and [str(int.from_bytes(row[0], "big")) for row in frames] == source["instruction_pointers"],
                            "SQLite stack differs from original WS Ask: " + kind)
                tids = database.execute("SELECT t.tid FROM event_actor_namespace_tids t JOIN events e ON e.header=t.header WHERE e.id=? ORDER BY t.ordinal", original).fetchall()
                require([row[0] for row in tids] == event["header"]["actor_namespace_tids"],
                        "SQLite namespace TIDs differ from original WS Ask")
                choices = database.execute(
                    "SELECT c.type,c.terminate FROM question_choices c JOIN questions q ON q.id=c.owner "
                    "WHERE q.decision_id=? ORDER BY c.ordinal", (bytes(identity),)).fetchall()
                require(choices == [("allow", None), ("deny", "none")], "SQLite Ask choices differ from WS")
                returned = database.execute(
                    "SELECT event_ref,operation,wait_id,returned_boottime_ns,return_value "
                    "FROM native_request_returns WHERE decision_id=?", (bytes(identity),)).fetchall()
                require(len(returned) == 1, "native request return missing or ambiguous")
                reference, operation, wait_id, returned_ns, result = returned[0]
                header = event["header"]
                event_key = {key: header[key] for key in ("source_instance", "sequence_domain", "sequence")}
                expected_operation = {("file", 1): "file_open", ("process", 1): "execute",
                                      ("network", 1): "connect", ("network", 2): "send"}[(header["kind"], header["operation"])]
                require(operation == expected_operation and json.loads(reference) == {"kind": "event", "value": event_key},
                        "native return crossed an original EventKey")
                require(int.from_bytes(wait_id, "big") > 0
                        and int.from_bytes(returned_ns, "big") >= int(header["source_boottime_ns"]),
                        "native return lost its wait identity or original order")
                verify_native_return(result, expected_errno, expected_return)
                request_returns.append({"event": event_key, "wait_id": int.from_bytes(wait_id, "big"), "value": result,
                                        "expected_value": -expected_errno if expected_errno else expected_return})
            rows = database.execute("SELECT d.cursor_runtime,d.cursor_sequence FROM chat_inputs i "
                                    "JOIN deliveries d ON d.id=i.delivery ORDER BY i.delivery").fetchall()
        committed = [{"service_instance": list(runtime), "sequence": str(int.from_bytes(sequence, "big"))}
                     for runtime, sequence in rows]
        start = committed.index(self.cursors[0])
        require(committed[start:start + len(self.cursors)] == self.cursors, "live and resumed cursors differ from SQLite")
        path = Path(self.args.service).resolve().parents[1] / "log/security.sqlite"
        with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True) as database:
            for original in self.security_records:
                row = database.execute("SELECT record FROM rule_logs WHERE id=?", (int(original["id"]),)).fetchone()
                require(row is not None and json.loads(row[0]) == original["record"],
                        "WS security page differs from committed original")
        return {"checked_questions": len(self.asks), "checked_cursors": len(self.cursors), "request_returns": request_returns,
                "checked_security_records": len(self.security_records)}

    def run(self):
        self.configure()
        self.case("native_service_start", self.start_service)
        self.case("ws_protection_management", self.protection)
        self.case("ws_original_ask_answer_effect", self.questions)
        self.case("ws_runtime_audit", self.runtime_audit)
        self.case("ws_write_close_audit", self.write_close_audit)
        self.case("ws_live_resume_history_activity", self.chat_workbench)
        self.case("ws_tool_attribution", self.tool_attribution)
        self.case("ws_read_only_authorization", self.readonly)
        self.case("actual_service_shutdown", self.stop_service)
        self.case("original_sqlite_evidence", self.audit_database)
        self.report["passed"] = True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("run",))
    parser.add_argument("--service", required=True)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--socket", default="/as/run/middleware.sock")
    args = parser.parse_args()
    if (Path(args.service).resolve() != Path("/as/bin/as-service")
            or Path(args.fixtures).resolve() != Path("/suite")
            or Path(args.socket) != Path("/as/run/middleware.sock")
            or "as_mvp_smoke=1" not in Path("/proc/cmdline").read_text().split()):
        parser.error("run the frontend from its packaged MVP guest")
    return run_suite(FrontendSuite(args))


if __name__ == "__main__":
    sys.exit(main())

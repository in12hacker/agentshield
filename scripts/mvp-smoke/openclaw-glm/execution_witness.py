"""Guest-only independent wait, return and local target effects."""

import json
import os
from pathlib import Path
import select
import shutil
import socket
import time

HERE = Path(__file__).resolve().parent
FIXTURE = json.loads((HERE / "observation-cases.json").read_text())
ROOT = Path(FIXTURE["base"])


def file_state(path, descriptor):
    file = Path(path)
    stat = os.fstat(descriptor)
    original = {
        "path": str(file),
        "inode": str(stat.st_ino),
        "device": [os.major(stat.st_dev), os.minor(stat.st_dev)],
        "mode": stat.st_mode,
        "bytes": list(os.pread(descriptor, stat.st_size, 0)),
    }
    try:
        current = file.stat()
        original["current_path"] = {
            "inode": str(current.st_ino),
            "device": [os.major(current.st_dev), os.minor(current.st_dev)],
            "bytes": list(file.read_bytes()),
        }
    except OSError as error:
        original["current_path"] = {"errno": error.errno, "error": str(error)}
    return original



class ExecutionWitness:
    def __init__(self):
        self.files = {}
        self.listeners = {}
        self.peers = {}
        for name in ("files", "images", "witness"):
            directory = ROOT / name
            directory.mkdir(mode=0o755, parents=True)
            os.chown(directory, 1000, 1000)
        for name, case in FIXTURE["cases"].items():
            if case.get("family") is None:
                continue
            if case["family"] == "file":
                path = Path(case["target"])
                path.write_text(case["initial"])
                os.chown(path, 1000, 1000)
                path.chmod(0o600)
                self.files[name] = os.open(path, os.O_RDONLY | os.O_CLOEXEC)
            elif case["family"] == "execute":
                shutil.copy2(Path(FIXTURE["native_program"]), case["target"])
                path = Path(case["witness"])
                path.write_text(FIXTURE["original_marker"])
                os.chown(path, 1000, 1000)
                path.chmod(0o600)
                self.files[name] = os.open(path, os.O_RDONLY | os.O_CLOEXEC)
            else:
                listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                listener.bind(("127.0.0.1", case["port"]))
                listener.listen(4)
                self.listeners[name] = listener

    def baseline(self, name):
        case = FIXTURE["cases"][name]
        if case["family"] in ("file", "execute"):
            return file_state(case.get("witness", case["target"]), self.files[name])
        listener = self.listeners[name]
        return {
            "listener": list(listener.getsockname()),
            "accept_pending": bool(select.select([listener], [], [], 0)[0]),
        }

    def after_answer(self, name, ask):
        # The real decision has already been sent. This optional witness must
        # never delay it and never claims to be a pre-answer held-thread sample.
        case = FIXTURE["cases"][name]
        if case["family"] == "send":
            listener = self.listeners[name]
            if name not in self.peers and select.select([listener], [], [], 0)[0]:
                self.peers[name] = listener.accept()
        return {"capture_phase": "after_answer", "observed_boottime_ns": time.clock_gettime_ns(time.CLOCK_BOOTTIME)}

    def finish(self, name):
        case = FIXTURE["cases"][name]
        if case["family"] in ("file", "execute"):
            effect = self.baseline(name)
        elif case["family"] == "connect":
            listener = self.listeners[name]
            effect = self.baseline(name)
            if effect["accept_pending"]:
                connection, peer = listener.accept()
                effect["peer"] = list(peer)
                connection.close()
        else:
            listener = self.listeners[name]
            if name not in self.peers and select.select([listener], [], [], 0)[0]:
                self.peers[name] = listener.accept()
            connection, peer = self.peers.get(name, (None, None))
            if not hasattr(self, "received"):
                self.received = {}
            collected = self.received.setdefault(name, bytearray())
            complete = False
            if connection is not None:
                connection.setblocking(False)
                while True:
                    try:
                        chunk = connection.recv(65536)
                    except BlockingIOError:
                        break
                    if not chunk:
                        complete = True
                        connection.close()
                        del self.peers[name]
                        break
                    collected.extend(chunk)
            effect = {
                "connected": connection is not None,
                "peer": list(peer) if peer else None,
                "stream_ended": complete,
                "received_bytes": list(collected),
            }
        return {"effect": effect}

    def close(self):
        for descriptor in self.files.values():
            os.close(descriptor)
        self.files.clear()
        for connection, _ in self.peers.values():
            connection.close()
        for listener in self.listeners.values():
            listener.close()
        return {
            "all_witness_descriptors_closed": not self.files,
            "all_listeners_closed": all(
                listener.fileno() == -1 for listener in self.listeners.values()
            ),
            "remaining_trace_instances": [],
        }

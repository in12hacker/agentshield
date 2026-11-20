"""Transport-only robustness tests; these do not claim AS/native protection."""
import importlib.util
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
from types import SimpleNamespace
import unittest

spec = importlib.util.spec_from_file_location("smoke_middleware", Path(__file__).with_name("middleware.py"))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ClientTests(unittest.TestCase):
    def run_server(self, frames, action):
        with tempfile.TemporaryDirectory() as root:
            endpoint = root + "/s"
            listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            listener.bind(endpoint)
            listener.listen(1)
            def serve():
                with listener.accept()[0] as channel:
                    for frame in frames:
                        raw = json.dumps(frame).encode() + b"\n"
                        for chunk in [raw[:5], raw[5:]]:
                            channel.sendall(chunk)
                listener.close()
            task = threading.Thread(target=serve)
            task.start()
            client = m.MiddlewareClient(endpoint, expected_pid=os.getpid(), expected_uid=os.geteuid())
            try:
                action(client)
            finally:
                client.close()
                task.join(timeout=2)
                self.assertFalse(task.is_alive())

    def test_interleaved_events_are_not_discarded_by_reply_wait(self):
        event = {"version": 2, "message": {"type": "driver_status", "snapshot": {"state_revision": 7}}}
        reply = {"version": 2, "message": {"type": "reply", "id": [3] * 16, "result": {"status": "completed"}}}
        def exercise(client):
            self.assertEqual(client.wait(lambda f:f["message"].get("id") == [3]*16), reply)
            self.assertEqual(client.wait(lambda f:f["message"].get("type") == "driver_status"), event)
        self.run_server([event, reply], exercise)
        business={"version":2,"business_reply":{"id":[4]*16,"result":{"status":"failed","code":"unauthorized","message":"audit query unavailable"}}}
        self.run_server([business],lambda client:self.assertEqual(client.wait(lambda frame:frame.get("business_reply",{}).get("id")==[4]*16),business))

    def test_private_response_never_enters_public_event_recorder(self):
        frame = {"version": 2, "private_reply": {"id": [5]*16, "result": {"status": "completed", "data": {"value": "private-phantom"}}}}
        records = []
        def exercise(client):
            client.recorder = records.append
            actual = client.wait(lambda f:"private_reply" in f)
            self.assertEqual(actual["private_reply"]["result"]["data"]["value"], "private-phantom")
        self.run_server([frame], exercise)
        self.assertNotIn("private-phantom", json.dumps(records))

    def test_an_already_failed_service_cannot_be_reported_as_successful_shutdown(self):
        suite = object.__new__(m.MvpSuite)
        suite.client = None
        suite.service = SimpleNamespace(poll=lambda: 1, wait=lambda **_: 1, pid=123)
        with self.assertRaises(m.SmokeError):
            suite.stop_service()

    def test_old_restart_markers_do_not_prove_current_retirement(self):
        with tempfile.TemporaryDirectory() as root:
            old = b"AS_SCRIPTED_SERVICE_STOPPED\nAS_SCRIPTED_MEDIA_SUMMARY not_forwarded=1\n"
            path = Path(root) / "service.log"
            path.write_bytes(old)
            suite = object.__new__(m.MvpSuite)
            suite.client = None
            suite.service = SimpleNamespace(poll=lambda: 0, wait=lambda **_: 0, pid=123)
            suite.output = Path(root)
            suite.patterns = []
            suite.service_log_start = len(old)
            with open(path, "ab") as log:
                suite.service_log = log
                with self.assertRaises(m.SmokeError):
                    suite.stop_service()

    def test_unknown_or_ambiguous_transport_is_not_accepted(self):
        bad = {"version": 2, "message": {"type":"reply"}, "private_reply": {"id":[]}}
        def exercise(client):
            with self.assertRaises(m.SmokeError):
                client.wait(lambda _: True)
        self.run_server([bad], exercise)


if __name__ == "__main__":
    unittest.main()

"""Acceptance-oracle robustness only; these checks do not run AS, an Agent or GLM."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from middleware import group, prefix, rule
from cases import (
    OriginalFrames,
    business_start_failed,
    credential_chains,
    effect_matches,
    gateway_startup_ready,
    scenarios,
    original_result,
    wait_for_original,
)


class AcceptanceChecks(unittest.TestCase):
    def test_original_survives_observer_errors_and_elapsed_time(self):
        from unittest.mock import patch

        name = "actual-case"
        observer_error = {"case": name, "type": "browser_error", "error": "observer detached"}
        terminal = {"case": name, "type": "terminal", "original": {"type": "event", "event": "chat", "payload": {"state": "final"}}}
        self.assertIsNone(original_result([observer_error], name))
        snapshots = iter([[observer_error], [observer_error], [observer_error, terminal]])
        with patch("time.monotonic", side_effect=[0, 100_000, 200_000]):
            actual = wait_for_original(lambda: next(snapshots), lambda rows: original_result(rows, name), idle=lambda _: None)
        self.assertIs(actual, terminal)

    def setUp(self):
        self.config, self.policy = scenarios("ordinary local task", rule, prefix, group)

    def test_stock_readiness_and_rpc_success_do_not_invent_a_started_run(self):
        self.assertFalse(gateway_startup_ready(200, {"ok": True}))
        self.assertFalse(gateway_startup_ready(503, {"status": "draining"}))
        self.assertTrue(gateway_startup_ready(200, {"status": "started"}))
        reply = {
            "type": "reply",
            "original": {
                "ok": True,
                "payload": {
                    "runStarted": False,
                    "runError": {
                        "code": "INVALID_REQUEST",
                        "message": "GatewayDrainingError",
                    },
                },
            },
        }
        self.assertTrue(business_start_failed(reply))
        self.assertFalse(
            business_start_failed(
                {
                    "type": "reply",
                    "original": {
                        "ok": True,
                        "payload": {"runStarted": True, "runId": "actual-run"},
                    },
                }
            )
        )

    def test_all_families_use_resource_scopes_and_normal_task_prompts(self):
        cases = self.config["cases"]
        self.assertEqual(
            {(c["family"], c["decision"]["type"]) for c in cases.values()},
            {
                (f, d)
                for f in ("file", "execute", "connect", "send")
                for d in ("allow", "deny")
            },
        )
        for item in self.policy["groups"]:
            for entry in item["rules"]:
                self.assertEqual(entry["source"], {})
                self.assertNotIn("pid", entry["target"])
                for match in (
                    entry["target"].get("path"),
                    entry["target"].get("executable"),
                ):
                    if match:
                        self.assertTrue(match["value"].endswith("/"))
                        self.assertEqual(match["operator"], "prefix")
        for case in cases.values():
            self.assertNotIn(self.config["deny_feedback_text"], case["prompt"])
            self.assertNotIn("拒绝立即停止", case["prompt"])
        sending = cases["send-deny"]
        self.assertEqual(
            sending["answers"][cases["connect-allow"]["rule_id"]], {"type": "allow"}
        )
        self.assertEqual(sending["answers"][sending["rule_id"]]["type"], "deny")

    def test_effects_follow_declared_family_and_decision_not_file_case_names(self):
        before = {"bytes": [1], "current_path": {"inode": "9", "bytes": [1]}}
        denied_process = self.config["cases"]["execute-deny"]
        self.assertTrue(effect_matches(denied_process, before, before, self.config))
        changed_fd = {**before, "bytes": [2]}
        self.assertFalse(
            effect_matches(denied_process, before, changed_fd, self.config)
        )
        connect = self.config["cases"]["connect-deny"]
        self.assertTrue(
            effect_matches(
                connect,
                {"accept_pending": False},
                {"accept_pending": False},
                self.config,
            )
        )
        self.assertIsNone(
            effect_matches(
                connect,
                {"accept_pending": True},
                {"accept_pending": False},
                self.config,
            )
        )
        sending = self.config["cases"]["send-deny"]
        self.assertIsNone(
            effect_matches(
                sending, {}, {"connected": False, "received_bytes": []}, self.config
            )
        )
        self.assertIsNone(
            effect_matches(
                sending,
                {},
                {"connected": True, "received_bytes": [], "stream_ended": False},
                self.config,
            )
        )
        self.assertTrue(
            effect_matches(
                sending,
                {},
                {"connected": True, "received_bytes": [], "stream_ended": True},
                self.config,
            )
        )
        self.assertFalse(
            effect_matches(
                sending,
                {},
                {"connected": True, "received_bytes": [1], "stream_ended": True},
                self.config,
            )
        )
        self.assertIsNone(
            effect_matches(
                denied_process, {"current_path": {}}, {"current_path": {}}, self.config
            )
        )

    def test_phantom_stages_cannot_be_spliced_between_attempts(self):
        process = {"host_tgid": 10, "birth_boottime_ns": "0", "boot_id": [3] * 16}

        def fact(sequence, state, revision):
            return {
                "id": {
                    "exchange": {"flow_instance": [1] * 16, "sequence": "8"},
                    "sequence": str(sequence),
                },
                "revision": str(revision),
                "state": state,
                "binding_ref": "binding",
                "actor": {"process": process},
                "destination_origin": "https://open.bigmodel.cn:443",
                "auth_field": "authorization",
            }

        split = [fact(1, "applied_to_request", 1), fact(2, "submitted", 2)]
        self.assertFalse(
            any(
                c["applied_then_submitted"]
                for c in credential_chains(split, "binding", process)
            )
        )
        joined = [fact(1, "applied_to_request", 1), fact(1, "submitted", 2)]
        self.assertTrue(
            credential_chains(joined, "binding", process)[0]["applied_then_submitted"]
        )
        self.assertEqual(
            credential_chains(joined, "binding", {**process, "birth_boottime_ns": "1"}),
            [],
        )

    def test_live_originals_are_consumed_once_and_partial_lines_are_retained(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "original.jsonl"
            reader = OriginalFrames()
            path.write_bytes(b'{"value":0}\n{"value":')
            self.assertEqual(list(reader.read(path)), [{"value": 0}])
            self.assertEqual(list(reader.read(path)), [])
            with path.open("ab") as stream:
                stream.write(b"false}\n")
            self.assertEqual(list(reader.read(path)), [{"value": False}])
            self.assertEqual(list(reader.read(path)), [])


if __name__ == "__main__":
    unittest.main()

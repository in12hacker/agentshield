"""Exercise fresh stock control requests; preserve actual native and business evidence."""

import json
from pathlib import Path
import subprocess
import sys
import time
import threading
from cases import OriginalFrames, business_start_failed, original_result, wait_for_original

fixture, out = map(Path, sys.argv[1:])
run = fixture / "run"
path = out / "live-report.json"
report = {"submitted": False, "asks": [], "cases": []}
last_diagnostic = 0
sys.path.insert(0, str(fixture / "root/suite"))
config = json.loads((out / "observation-cases.json").read_text())
seen_asks = set()
original_frames = OriginalFrames()
diagnostic_thread = None
diagnostic_results = []


class SutProcessExited(RuntimeError):
    """An acquired process exit, not a diagnostic or elapsed-time inference."""
if path.exists():
    raise SystemExit("owned run already recorded")


def save():
    temp = path.with_suffix(".tmp")
    temp.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    temp.replace(path)


def guest(op, **values):
    result = subprocess.run(
        [sys.executable, str(fixture / "call-vm.py"), json.dumps({"op": op, **values})],
        capture_output=True,
        text=True,
        check=True,
    )
    reply = json.loads(result.stdout)
    if not reply["ok"]:
        raise RuntimeError(reply["error"])
    return reply["result"]


def capture_query(records, query, **metadata):
    record = {"query": query, **metadata}
    try:
        record["reply"] = guest("group_query", query=query)
    except (
        OSError,
        RuntimeError,
        ValueError,
        KeyError,
        subprocess.SubprocessError,
    ) as error:
        record["error"] = f"{type(error).__name__}: {error}"
    records.append(record)
    save()


def attempt(label, action):
    try:
        return action()
    except (
        OSError,
        RuntimeError,
        ValueError,
        KeyError,
        subprocess.SubprocessError,
    ) as error:
        report.setdefault("observation_errors", []).append(
            {"stage": label, "error": f"{type(error).__name__}: {error}"}
        )
        save()


def events():
    source = run / "browser-events.jsonl"
    if not source.exists():
        return []
    rows = []
    try:
        for line in source.read_text().splitlines(keepends=True):
            if line.endswith("\n"):
                try:
                    rows.append(json.loads(line))
                except ValueError as error:
                    report["browser_evidence_gap"] = str(error)
    except OSError as error:
        report["browser_evidence_gap"] = str(error)
    return rows


def send(value):
    browser.stdin.write(json.dumps(value, ensure_ascii=False) + "\n")
    browser.stdin.flush()


def wait(predicate, watch=None):
    return wait_for_original(events, predicate, watch)


def configure_review():
    guest("enable_review")
    document = json.loads((out / "rule-plan.json").read_text())
    prior = guest("request", kind="rules", command={"type": "get"})["snapshot"]
    document["groups"] = prior["document"]["groups"] + document["groups"]
    report["actual_proposed_rules"] = document
    save()
    adoption = guest(
        "request",
        kind="rules",
        command={
            "type": "update",
            "expected_version": prior["version"],
            "document": document,
        },
    )
    actual = guest("request", kind="rules", command={"type": "get"})["snapshot"]
    version = adoption["receipt"]["saved_version"]
    if actual["version"] != version or adoption["applied"]["rule_version"] != version:
        raise RuntimeError("actual rule adoption differs")
    report["policy"] = {"adoption": adoption, "effective": actual}
    save()
    return version


def _observe_ask(name, version):
    global last_diagnostic, diagnostic_thread
    while diagnostic_results:
        diagnostic = diagnostic_results.pop(0)
        if "diagnostic_error" in diagnostic:
            report.setdefault("diagnostic_errors", []).append(diagnostic)
            continue
        with (out / "process-diagnostics.jsonl").open("a") as stream:
            stream.write(json.dumps(diagnostic) + "\n")
        if diagnostic.get("AS_exit") is not None or diagnostic.get("OpenClaw_exit") is not None:
            report["source_process_exit"] = diagnostic.get("AS_exit")
            report["target_process_exit"] = diagnostic.get("OpenClaw_exit")
            (out / "runtime-diagnostic.json").write_text(
                json.dumps({"ok": True, "result": diagnostic}, indent=2) + "\n"
            )
            save()
            raise SutProcessExited(
                "AS/OpenClaw actually exited; retain the original pending result"
            )
    source = run / "live-as-events.jsonl"
    case = config["cases"][name]
    for original in original_frames.read(source):
        ask = original.get("message", {})
        if ask.get("type") != "ask":
            continue
        key = bytes(ask["decision_id"])
        if key in seen_asks:
            continue
        seen_asks.add(key)
        if (
            ask["hit"]["rule_id"] not in case["answers"]
            or ask["hit"]["source"]["version"] != version
        ):
            report.setdefault("other_asks", []).append(ask)
            continue
        decision = case["answers"][ask["hit"]["rule_id"]]
        item = {"case": name, "ask": ask, "decision": decision}
        report["asks"].append(item)
        save()
        item["response"] = attempt(
            "answer " + name,
            lambda: guest("answer", case=name, ask=ask, decision=decision),
        )
        save()
    # Optional log snapshots never sit in front of an original Ask answer.
    if (diagnostic_thread is None or not diagnostic_thread.is_alive()) and time.monotonic() - last_diagnostic >= 10:
        last_diagnostic = time.monotonic()

        def read_diagnostic():
            try:
                diagnostic_results.append(guest("diagnostic"))
            except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
                diagnostic_results.append({"diagnostic_error": str(error)})

        diagnostic_thread = threading.Thread(target=read_diagnostic, daemon=True)
        diagnostic_thread.start()
    if original_frames.issues:
        report["AS_evidence_gaps"] = original_frames.issues.copy()


def observe_ask(name, version):
    try:
        _observe_ask(name, version)
    except SutProcessExited:
        raise
    except Exception as error:
        report.setdefault("diagnostic_errors", []).append(f"Ask observation: {error}")
        save()


def operation_case(name, version):
    row = {"name": name, "submitted": False}
    report["cases"].append(row)
    try:
        diagnostic = guest("diagnostic")
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        diagnostic = {}
        report.setdefault("diagnostic_errors", []).append(str(error))
    row["process_status_before_prepare"] = {
        k: diagnostic.get(k)
        for k in ("AS_exit", "OpenClaw_exit", "last_confirmed_phase")
    }
    if (
        diagnostic.get("AS_exit") is not None
        or diagnostic.get("OpenClaw_exit") is not None
    ):
        report["source_process_exit"] = diagnostic.get("AS_exit")
        report["target_process_exit"] = diagnostic.get("OpenClaw_exit")
        (out / "runtime-diagnostic.json").write_text(
            json.dumps({"ok": True, "result": diagnostic}, indent=2) + "\n"
        )
        save()
        raise RuntimeError(
            "Owned AS/OpenClaw has exited; no UI or business request submitted"
        )
    send({"op": "prepare", "pages": [name]})
    row["prepared"] = wait(
        lambda rows: next(
            (
                r
                for r in rows
                if r.get("page") == name and r["type"] in ("prepared", "browser_error")
            ),
            None,
        ),
        lambda: observe_ask(name, version),
    )
    if not row["prepared"] or row["prepared"]["type"] != "prepared":
        raise RuntimeError("scenario page did not prepare")
    try:
        row["baseline"] = guest("case_baseline", case=name)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        row["baseline_observation_error"] = str(error)
    row["submitted"] = True
    report["submitted"] = True
    save()
    send(
        {
            "op": "submit",
            "cases": [
                {
                    "name": name,
                    "page": name,
                    "follow": False,
                    "prompt": config["cases"][name]["prompt"],
                }
            ],
        }
    )
    row["completion"] = wait(
        lambda rows: original_result(rows, name),
        lambda: observe_ask(name, version),
    )
    observe_ask(name, version)
    if row["completion"] and business_start_failed(row["completion"]):
        row["business_start_failure"] = row["completion"]["original"]
        save()
        return False
    completion = row["completion"] or {}
    original = completion.get("original", {})
    if (
        completion.get("type") != "terminal"
        or original.get("type") != "event"
        or original.get("event") != "chat"
        or original.get("payload", {}).get("state") not in ("final", "error", "aborted")
    ):
        report["subsequent_business_not_run"] = (
            "original request has no observed Agent terminal; retain it and do not submit another case"
        )
        save()
        return False
    try:
        row["after"] = guest("case_finish", case=name)
        while (config["cases"][name]["family"] == "send"
               and row["after"].get("effect", {}).get("connected") is True
               and row["after"]["effect"].get("stream_ended") is False):
            save()
            observe_ask(name, version)
            time.sleep(0.2)
            row["after"] = guest("case_finish", case=name)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        row["effect_observation_error"] = str(error)
    save()
    print(name, (row["completion"] or {}).get("type"), flush=True)
    return True


browser = None
save()
try:
    while True:
        status = json.loads((run / "status.json").read_text())
        if status["state"] == "ready":
            break
        if status["state"] in ("failed", "ended", "initialization_failed"):
            raise RuntimeError("owned guest failed before submission")
        time.sleep(1)
    report["review"] = guest("activate_media")
    with (
        (out / "browser-worker.log").open("x") as log,
        (out / "browser-worker.stderr").open("x") as err,
    ):
        browser = subprocess.Popen(
            ["node", str(fixture / "agent-browser.cjs")],
            stdin=subprocess.PIPE,
            stdout=log,
            stderr=err,
            text=True,
        )
    version = configure_review()
    for name in config["cases"]:
        completed = attempt(name, lambda: operation_case(name, version))
        if completed is not True:
            # No new business while an original is unresolved. A failed start
            # with an actual reply is settled and does not disable other cases.
            row = report["cases"][-1]
            if row.get("submitted") and not row.get("business_start_failure"):
                report.setdefault("subsequent_business_not_run", "original unresolved; no new submission")
                save()
                break
            save()
        if (
            "source_process_exit" in report
            or "target_process_exit" in report
            or "subsequent_business_not_run" in report
        ):
            break
        final = (
            (report["cases"][-1].get("completion") or {}).get("original") or {}
        ).get("payload", {})
        # A settled failed Agent round is recorded on this case. Other independent
        # controls may still run while the actual AS/Gateway processes remain alive.
    report["security"] = attempt("security page", lambda: guest("security_page"))
    report["event_activities"] = []
    for item in report["asks"]:
        h = item["ask"]["event"]["header"]
        event = {k: h[k] for k in ("source_instance", "sequence_domain", "sequence")}
        capture_query(
            report["event_activities"],
            {
                "type": "activity_context",
                "anchor": {"kind": "event", "value": event},
                "attribution": None,
            },
            case=item["case"],
        )
except KeyboardInterrupt:
    report["operator_stop_requested"] = True
    save()
except (
    OSError,
    RuntimeError,
    ValueError,
    KeyError,
    subprocess.SubprocessError,
) as error:
    report["error"] = f"{type(error).__name__}: {error}"
    print(report["error"], flush=True)
finally:
    # A failed observer is not permission to cancel a healthy in-flight Agent.
    # Keep the original and its Ask handling alive until a genuine end or stop.
    if report.get("cases"):
        active = report["cases"][-1]
        while (active.get("submitted") and not report.get("operator_stop_requested")
               and "source_process_exit" not in report and "target_process_exit" not in report):
            completion = active.get("completion") or original_result(events(), active["name"])
            if completion is not None:
                active["completion"] = completion
                break
            try:
                observe_ask(active["name"], version)
            except SutProcessExited:
                break
            except KeyboardInterrupt:
                report["operator_stop_requested"] = True
                break
            except Exception as error:
                report["observer_error_while_original_pending"] = str(error)
            time.sleep(0.2)
    if browser:
        try:
            if browser.poll() is None:
                try:
                    send({"op": "close"})
                finally:
                    browser.stdin.close()
        except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
            report["browser_shutdown_error"] = f"{type(error).__name__}: {error}"
            report.setdefault("error", report["browser_shutdown_error"])
        try:
            report["browser_exit"] = browser.wait()
        except (OSError, RuntimeError, subprocess.SubprocessError) as error:
            report["browser_join_error"] = f"{type(error).__name__}: {error}"
            report.setdefault("error", report["browser_join_error"])
    report.setdefault("browser_exit", None)
    save()
raise SystemExit(
    bool(
        report.get("error")
        or report.get("subsequent_business_not_run")
        or report.get("browser_exit") not in (None, 0)
    )
)

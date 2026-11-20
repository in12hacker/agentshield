"""Claim-scoped real GLM/AS phantom evidence analysis; missing native stages are not business gates."""

from pathlib import Path
import json, sqlite3, hashlib, re, sys, urllib.parse
from cases import effect_matches, credential_chains

r = Path(sys.argv[1])
evidence_gaps = []


def read(name):
    try:
        return json.loads((r / name).read_text())
    except (OSError, ValueError) as error:
        evidence_gaps.append({"source": name, "error": str(error)})
        return {}


def lines(name):
    try:
        source = (r / name).read_text().splitlines()
    except OSError as error:
        evidence_gaps.append({"source": name, "error": str(error)})
        return []
    result = []
    for number, line in enumerate(source, 1):
        try:
            result.append(json.loads(line))
        except ValueError as error:
            evidence_gaps.append({"source": name, "line": number, "error": str(error)})
    return result


report = read("live-report.json")
status = read("status.json")
scope = read("provider-scope.json")
try:
    conn = sqlite3.connect(f"file:{r}/analysis-sqlite/business.sqlite?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
except sqlite3.Error as error:
    conn = None
    evidence_gaps.append({"source": "analysis-sqlite/business.sqlite", "error": str(error)})


def sql(statement, parameters=()):
    if conn is None:
        return None
    try:
        return conn.execute(statement, parameters).fetchall()
    except sqlite3.Error as error:
        evidence_gaps.append({"source": "SQL", "statement": statement, "error": str(error)})
        return None


observations = []
credentials = []
feedback = []
raw_events = []
initial = status.get("initialization", {}).get("result", {})
actual_binding = initial.get("binding_id")
actual_actor = initial.get("gateway_key")


def is_GLM_origin(value):
    try:
        u = urllib.parse.urlsplit(value or "")
        return (
            u.scheme == "https"
            and u.hostname == "open.bigmodel.cn"
            and u.port in (None, 443)
            and u.path in ("", "/")
            and not u.query
            and not u.fragment
            and not u.username
            and not u.password
        )
    except ValueError:
        return False


def is_actual_GLM_source(source):
    return (
        actual_actor is not None
        and source.get("actor", {}).get("process") == actual_actor
        and is_GLM_origin(source.get("origin"))
    )


def body_bytes(b):
    if not isinstance(b, dict):
        return None
    declared = b.get("original_length")
    end = int(declared) if declared is not None else None
    pieces = sorted(b.get("segments", []), key=lambda x: int(x.get("offset", "0")))
    data = bytearray()
    offset = 0
    for s in pieces:
        if s.get("kind") != "bytes" or int(s.get("offset", "0")) != offset:
            break
        chunk = bytes(s["data"])
        data.extend(chunk)
        offset += len(chunk)
    if end is not None and offset != end:
        evidence_gaps.append({"source": "acquired body", "declared_length": end, "contiguous_prefix_bytes": offset})
    return bytes(data)


for original in lines("as-events.jsonl"):
    e = original.get("message", {}).get("envelope", {}).get("event", {})
    z = e.get("data")
    if e.get("type") == "observation" and isinstance(z, dict):
        data = body_bytes(z.get("body"))
        parsed = None
        if data is not None:
            try:
                if z.get("unit") == "sse_event":
                    data_fields = [
                        line[5:].removeprefix(b" ")
                        for line in data.splitlines()
                        if line.startswith(b"data:")
                    ]
                    parsed = (
                        json.loads(b"\n".join(data_fields)) if data_fields else None
                    )
                else:
                    parsed = json.loads(data)
            except (ValueError, UnicodeDecodeError):
                pass
        observations.append(
            {
                "original": z,
                "parsed": parsed,
                "body_complete": (len(data) == int(z["body"]["original_length"]) if data is not None and z.get("body", {}).get("original_length") is not None else None),
                "acquired_body_sha256": hashlib.sha256(data).hexdigest()
                if data is not None
                else None,
            }
        )
    if e.get("type") == "credential_use":
        credentials.append(z)
    if e.get("type") == "rule_feedback":
        feedback.append(z)
models = set()
model_records = []
profiles = set()
origins = set()
client_tool_outputs = []
provider_errors = []
for o in observations:
    z = o["original"]
    p = o["parsed"]
    s = z["source"]
    profiles.add(s.get("profile"))
    origins.add(s.get("origin"))
    if isinstance(p, dict):
        if s.get("side") in ("server_response", "upstream_response"):
            response_models = [
                p.get("model"),
                p.get("response", {}).get("model")
                if isinstance(p.get("response"), dict)
                else None,
            ]
            for model in response_models:
                if isinstance(model, str) and is_actual_GLM_source(s):
                    models.add(model)
                    model_records.append(
                        {
                            "record": z["id"],
                            "model": model,
                            "actor": s["actor"]["process"],
                            "origin": s["origin"],
                            "side": s["side"],
                        }
                    )
            if p.get("error"):
                provider_errors.append({"record": z["id"], "error": p["error"]})
        if s.get("side") == "client_request":
            for x in p.get("messages", []):
                if isinstance(x, dict) and x.get("role") == "tool":
                    client_tool_outputs.append(
                        {
                            "record": z["id"],
                            "api": "chat_completions",
                            "call_id": x.get("tool_call_id"),
                            "output": x.get("content"),
                            "body_sha256": o["acquired_body_sha256"],
                            "actor": s.get("actor", {}).get("process"),
                            "origin": s.get("origin"),
                        }
                    )
            for x in p.get("input", []) if isinstance(p.get("input", []), list) else []:
                if isinstance(x, dict) and x.get("type") == "function_call_output":
                    client_tool_outputs.append(
                        {
                            "record": z["id"],
                            "api": "responses",
                            "call_id": x.get("call_id"),
                            "output": x.get("output"),
                            "body_sha256": o["acquired_body_sha256"],
                            "actor": s.get("actor", {}).get("process"),
                            "origin": s.get("origin"),
                        }
                    )
browser = []
p = r / "browser-events.jsonl"
if p.exists():
    browser = lines("browser-events.jsonl")
config = read("observation-cases.json")
ws_completions = {}
for original in lines("ws-frames.jsonl"):
    message = original.get("frame", {}).get("message", {})
    if original.get("direction") == "in" and message.get("type") == "operation_result":
        ws_completions[json.dumps(message.get("decision_id"))] = message
case_results = []
for case in report.get("cases", []):
    name = case["name"]
    questions = [x for x in report.get("asks", []) if x["case"] == name]
    receipts = []
    for x in questions:
        q = next(
            (
                y
                for y in report.get("event_activities", [])
                if y.get("query", {}).get("anchor", {}).get("value")
                == {
                    k: x["ask"]["event"]["header"][k]
                    for k in ("source_instance", "sequence_domain", "sequence")
                }
            ),
            {},
        )
        records = (
            q.get("reply", {}).get("data", {}).get("view", {}).get("source_records", [])
        )
        op = next(
            (
                y["value"]
                for y in records
                if y["kind"] == "operation_result"
                and y["value"]["decision_id"] == x["ask"]["decision_id"]
            ),
            ws_completions.get(json.dumps(x["ask"]["decision_id"])),
        )
        rows = sql(
            "select proposal_type,proposal_terminate,proposal_reason,outcome from operation_reports where decision_id=?",
            (bytes(x["ask"]["decision_id"]),),
        )
        expected = "allowed" if x["decision"]["type"] == "allow" else "denied"
        ok = bool(
            op
            and op["result"].get("proposal", {}).get("decision") == x["decision"]
            and op["result"]["proposal"].get("reason") == "user"
            and op["result"].get("outcome") == expected
        )
        sql_ok = None if rows is None else bool(rows) and all(
                row["proposal_type"] == x["decision"]["type"]
                and row["proposal_terminate"] == x["decision"].get("terminate")
                and row["proposal_reason"] == "user"
                and row["outcome"] == expected
                for row in rows
            )
        receipts.append(
            {
                "decision_id": x["ask"]["decision_id"],
                "event": {
                    k: x["ask"]["event"]["header"][k]
                    for k in ("source_instance", "sequence_domain", "sequence")
                },
                "RuleHit": x["ask"]["hit"],
                "original_user_receipt": op["result"] if op else None,
                "original_WS_user_receipt": ok,
                "SQL_user_receipt": sql_ok,
                "SQL_and_original_WS_user_receipt": ok and sql_ok is True,
            }
        )
    tools = [
        x["original"]["payload"]["data"]
        for x in browser
        if x.get("case") == name
        and x["type"] == "tool"
        and x["original"]["payload"]["data"].get("phase") == "result"
    ]
    starts = [
        x["original"]["payload"]["data"]
        for x in browser
        if x.get("case") == name
        and x["type"] == "tool"
        and x["original"]["payload"]["data"].get("phase") == "start"
    ]
    last = tools[-1] if tools else None
    text = "\n".join(
        p.get("text", "")
        for p in (last or {}).get("result", {}).get("content", [])
        if p.get("type") == "text"
    )
    tool_reference = (last or {}).get("toolCallId")
    matching_outputs = [
        x
        for x in client_tool_outputs
        if tool_reference is not None
        and x["call_id"] == tool_reference
        and actual_actor is not None
        and x["actor"] == actual_actor
        and is_GLM_origin(x["origin"])
        and text
        and x["output"] == text
    ]
    completion = case.get("completion") or {}
    original = completion.get("original", {})
    complete = original.get("payload", {})
    final = (
        completion.get("type") == "terminal"
        and original.get("type") == "event"
        and original.get("event") == "chat"
        and complete.get("state") == "final"
    )
    declared = config.get("cases", {}).get(name, {})
    before = case.get("baseline", {})
    after = case.get("after", {}).get("effect", {})
    try:
        effect = effect_matches(declared, before, after, config)
    except (KeyError, TypeError, ValueError) as error:
        effect = None
        evidence_gaps.append({"source": name + " physical effect", "error": str(error)})
    relevant = [
        x for x in questions if x["ask"]["hit"]["rule_id"] == declared.get("rule_id")
    ]
    control_ok = (
        bool(relevant)
        and all(
            x["decision"] == declared.get("answers", {}).get(x["ask"]["hit"]["rule_id"])
            for x in questions
        )
        and all(x["original_WS_user_receipt"] for x in receipts)
        and effect is True
    )
    case_results.append(
        {
            "case": name,
            "business": "PASS"
            if control_ok
            else ("NOT_RUN" if not questions else "NOT_PROVEN"),
            "family": declared.get("family"),
            "actual_question_count": len(questions),
            "agent_round_complete": final,
            "agent_round_settled": complete.get("state") in ("final", "error", "aborted"),
            "SQL_persistence": "PASS" if receipts and all(x["SQL_user_receipt"] is True for x in receipts) else "NOT_PROVEN",
            "tool_error_reported": (last or {}).get("isError"),
            "receipts": receipts,
            "actual_tool_results": tools,
            "exact_tool_reference_and_text_observed_in_followup": bool(
                matching_outputs
            ),
            "observed_followup_records": matching_outputs,
            "final_state": complete.get("state"),
            "final_message": complete.get("message"),
            "target_effect_verified": effect,
            "attempted_content_from_actual_ToolCall": starts,
            "post_feedback_submitted_to_vendor": "NOT_PROVEN unless an actual Submitted fact/owned-upstream receipt is present; original observation alone is not post-rewrite wire proof",
        }
    )
recorded = {x["case"] for x in case_results}
for name, declared in config.get("cases", {}).items():
    if name not in recorded:
        case_results.append(
            {
                "case": name,
                "family": declared["family"],
                "business": "NOT_RUN",
                "actual_question_count": 0,
                "agent_round_complete": False,
                "reason": "no original business submission for this scenario",
            }
        )
rule_text = config.get("deny_feedback_text")
fixed_in_outputs = [
    x
    for x in client_tool_outputs
    if rule_text and isinstance(x["output"], str) and rule_text in x["output"]
]
tables = {
    x[0] for x in (sql("select name from sqlite_master where type='table'") or [])
}
counts = {
    n: (sql('select count(*) from "' + n + '"') or [[None]])[0][0]
    for n in (
        "questions",
        "operation_reports",
        "native_tool_executions",
        "feedback",
        "feedback_bindings",
    )
    if n in tables
}
try:
    log = (r / "as-service.log").read_text(errors="replace")
except OSError as error:
    log = ""
    evidence_gaps.append({"source": "as-service.log", "error": str(error)})
retire = (
    read("guest-retirement.json").get("result", {})
    if (r / "guest-retirement.json").exists()
    else {}
)
normal = (
    retire.get("as_exit") == 0
    and retire.get("oc_exit") == 0
    and status.get("qemu_exit") == 0
    and status.get("qemu_joined")
    and not status.get("forced_qemu_cleanup")
    and not (r / "abnormal-vm-stop.json").exists()
)
chains = (
    credential_chains(credentials, actual_binding, actual_actor)
    if actual_binding and actual_actor
    else []
)
proof_states = {state for chain in chains for state in chain["states"]}
phantom_proof = any(chain["applied_then_submitted"] for chain in chains)
result = {
    "scope": scope,
    "business_cases": case_results,
    "setup_error": report.get("error"),
    "business_status": "PASS"
    if case_results and len(case_results) == len(config.get("cases", {}))
    and all(x["business"] == "PASS" for x in case_results)
    else (
        "NOT_RUN"
        if not any(x["actual_question_count"] for x in case_results)
        else "NOT_PROVEN"
    ),
    "formal_phantom_replacement": {
        "status": "PASS" if phantom_proof else "NOT_PROVEN",
        "original_same_binding_actor_authorization_states": sorted(proof_states),
        "individual_attempts": chains,
        "scope": "cohort transport only; auxiliary model replies do not prove a business round",
        "actual_GLM_response_models_separate": sorted(models),
        "key_values_never_in_report": True,
    },
    "observed_real_provider_response_models": sorted(models),
    "actual_observed_profiles": sorted(x for x in profiles if x),
    "actual_observed_origins": sorted(x for x in origins if x),
    "credential_use_states": dict(
        __import__("collections").Counter(str(x.get("state")) for x in credentials)
    ),
    "credential_use_origins": sorted(
        {x.get("destination_origin", "") for x in credentials}
    ),
    "observed_tool_result_uplink_records": len(client_tool_outputs),
    "fixed_text_occurrences_in_original_uplink": None if rule_text is None else sum(
        x["output"].count(rule_text) for x in fixed_in_outputs
    ),
    "actual_feedback_events": feedback,
    "SQL_counts": counts,
    "provider_errors": provider_errors,
    "supported_positive_feedback": "NOT_PROVEN for stock unless trusted ToolExecution/ChatPartRef and actual write facts exist; no manual provenance injection",
    "candidate_no_rewrite_observation": "PASS"
    if client_tool_outputs
    and rule_text is not None
    and not fixed_in_outputs
    and counts.get("native_tool_executions") == 0
    and not feedback
    else "NOT_PROVEN",
    "normal_retirement": "PASS" if normal else "NOT_PROVEN",
    "native_receive_error_lines": [
        x for x in log.splitlines() if "AS_NATIVE_RECEIVE_FAILED" in x
    ],
    "business_write_error_lines": [
        x for x in log.splitlines() if "AS_BUSINESS_WRITE_FAILED" in x
    ],
    "frontends": {
        "AS_scripted_WS": report.get("review") is not None,
        "AS_Dashboard": "NOT_RUN",
        "OpenClaw_native_GUI_pages_prepared": sum(
            (x.get("prepared") or {}).get("type") == "prepared"
            for x in report.get("cases", [])
        ),
    },
    "not_substituted": [
        "direct readiness is not formal phantom acceptance",
        "software/localTLS is not stock or real GLM positive",
        "mode/candidate is not authorization",
        "retained original request bytes are not proof of post-rewrite vendor receipt",
    ],
}
result["actual_GLM_response_model_records"] = model_records
if (r / "observer-gaps.jsonl").exists():
    evidence_gaps.extend({"source": "optional_browser_observation", "original": value} for value in lines("observer-gaps.jsonl"))
result["evidence_gaps"] = evidence_gaps
result["normal_chain_status"] = (
    "PASS" if case_results
    and all(x["business"] == "PASS" and x["agent_round_complete"] for x in case_results)
    and phantom_proof and models else "NOT_PROVEN"
)
result["SQL_persistence_status"] = (
    "PASS" if case_results and all(x.get("SQL_persistence") == "PASS" for x in case_results)
    else "NOT_PROVEN"
)
result["verification_exit_semantics"] = "Report generation only; independent product outcomes are in this document."
(r / "glm-result.json").write_text(
    json.dumps(result, indent=2, ensure_ascii=False) + "\n"
)
(r / "observed-provider-records.json").write_text(
    json.dumps(
        {
            "observation_count": len(observations),
            "models": sorted(models),
            "actual_GLM_response_model_records": model_records,
            "client_tool_outputs": client_tool_outputs,
            "provider_errors": provider_errors,
        },
        indent=2,
        ensure_ascii=False,
    )
    + "\n"
)
print(
    json.dumps(
        {
            "business": result["business_status"],
            "cases": [(x["case"], x["business"]) for x in case_results],
            "actual_models": sorted(models),
            "profiles": result["actual_observed_profiles"],
            "credential_states": result["credential_use_states"],
            "fixed_text_in_original_uplink": result[
                "fixed_text_occurrences_in_original_uplink"
            ],
            "normal_retirement": result["normal_retirement"],
            "setup_error": report.get("error"),
        },
        ensure_ascii=False,
    )
)

raise SystemExit(0)

# Scripted MVP guest

This entry packages the real `as-service` binary as `/as/bin/as-service` and a Python middleware at `/suite/middleware.py`. All rules, configuration, answers and queries use the actual authenticated Unix socket `/as/run/middleware.sock`; this entry does not replay `native_service_smoke`, select a software driver, operate OpenClaw or test MCP.

The caller supplies a current matching Native build (produced by `drivers/linux/build.sh all`) and a readable matching kernel. Packaging compiles the current static GNU service, the new `mvp_tls_fixture`, and the existing `subject`, `rule_kill_probe`, `self_probe` fixtures. The default config grants the guest root middleware management/content access, includes the editable [Driver fast-allow preset](../../config/rules.fast-allow.yaml), and starts with no flow or credential bindings. The preset is copied to the existing `config/rules.yaml`; the rule owner parses, persists and compiles it through the ordinary path. The Native module, BPF objects and both static loaders are copied byte for byte from that build. Middleware creates its fixtures before starting the service and changes policy through the socket.

```sh
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
bash scripts/mvp-smoke/package.sh /tmp/NEW-MVP-PACKAGE /tmp/as-tls-native-build /bin/busybox /tmp/as-refactor-target
python3 scripts/mvp-smoke/run-guest.py /tmp/NEW-MVP-PACKAGE \
  --kernel /tmp/as-tls-native-main-r8-local-run/Image --output /tmp/NEW-MVP-RUN
```

Freeze current source before packaging and do not overwrite prior packages/runs. Packaging uses umask022, fixed root-owned protected directories without group/other write permission, and records their actual newc uid/gid/mode in package.json; backend guards stay enabled. These commands never load host modules, change host nft rules/trust or access a Root broker. QEMU TCG runs as the ordinary host UID; its guest root performs real kernel operations. The existing ARM Image SHA256 is `6220c8a5fca3340de6e0b3b7da5c834d1ff2868c90f2eeb171695ec393df45c6`. A different kernel requires its own matching source/artifacts and loading conditions; compilation is not protection evidence.

The distro `/usr` CPython executable, standard library and native extensions retain their normal paths. Dynamic dependencies are copied from trusted installed ELF inputs with `ldd`; user/site packages, pip, GUI and test directories are excluded. No interpreter fallback or virtual environment is created. The guest runs `python3 -I -B`, prepares its normal loopback hosts, and loads the matching distro RTC and netfilter dependencies before the AS module. It does not disable TLS verification or manually choose a certificate-friendly time.

The guest invokes exactly:

```text
python3 -I -B /suite/middleware.py run --service /as/bin/as-service --fixtures /suite --output /suite/mvp-report
```

Middleware exclusively creates a new 0700 report directory and owns starting, stopping and restarting its real service and fixtures. Its public provider CA stays in its own /suite fixture directory and SSL_CERT_FILE is passed only to the owned service child; the host and guest system trust stores are not changed. The thin service emits `AS_SCRIPTED_SERVICE_READY` only after real Native initialization and socket binding; SIGTERM/SIGINT drains it to `AS_SCRIPTED_SERVICE_STOPPED`. A not-implemented command is not a passing capability. The safety-log smoke reads the fixed backend SQLite file after service retirement. It compares the stored rule hit and original operation receipt and checks that log-disabled rules produced no rows. LLM and interaction business records use the middleware's fixed structured SQLite database; no backend chat replay or snapshot exists.

The current `subject` interface explicitly selects `open`, `mkdir`, `unlink`, `rmdir`, `rename`, `exchange`, `truncate-path`, `truncate-fd`, `truncate-open` or `map-shared-write`; all report the actual `operation`, `result` and `errno`. The file mutation case applies rules to its whole resource directory, without PID/name/argument match restrictions. It checks Driver Deny and original Ask/Allow for mkdir, unlink, rmdir, rename and exchange, retains original paths/device/inode/rename flags/mkdir mode, and compares the Ask's entire event and hit with its completion. The mkdir subject sets its own umask to zero and requests 0755; Linux's actual `path_mkdir` mode is checked as 0755, without claiming this is the resulting inode mode. While a directory operation is waiting with VFS locks held, the consumer only checks child liveness and answers through the existing channel; filesystem effects are inspected after the operation returns. The same case checks Driver Deny and original Ask/Allow/Deny for path truncate, FD truncate, truncate on open and a new shared writable mapping. It compares the actual file effects and original hook fields. Ordinary file pre-create and existing-FD Write are outside this case. See the [original file-gate run](../../docs/evidence/file-gate-integration.md) and [truncate and mapping evidence](../../docs/evidence/file-wait-extension.md).

Middleware's `report.json` uses `{passed: bool, cases: [{name, status, required?, ...}]}` with `PASS`, `FAIL` or `NOT_IMPLEMENTED`. Missing `required` means true. Required cases must all PASS; optional `required:false` future-scope cases are reported separately and NOT_IMPLEMENTED never becomes PASS. MCP is explicitly excluded rather than emitted as a passing case. Guest init exports that exact JSON in a serial report block, then requires actual module unload. The host runner preserves `qemu-full.log`, `middleware-report.json` and a final `report.json` containing the real host UID, kernel/image hashes, VM exit, timeout and child-join outcome. Success requires a passing middleware report, all required cases passing, guest exit0, actual Native unload and QEMU exit0. It does not invent external Root supervisor receipts or treat a missing report as success.

## Scripted WebSocket front end

```sh
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
bash scripts/mvp-smoke/package.sh --frontend-ws /tmp/NEW-WS-PACKAGE \
  /tmp/as-tls-native-build /bin/busybox /tmp/as-refactor-target
python3 scripts/mvp-smoke/run-guest.py /tmp/NEW-WS-PACKAGE \
  --kernel /tmp/as-tls-native-main-r8-local-run/Image --output /tmp/NEW-WS-RUN
```

The package runs `frontend.py` with the locked Tungstenite client built as `mvp_ws_client`. The guest uses only loopback networking. Workflows cover WS authentication and protection management, original Ask/answer/effect, runtime audit, and chat live delivery/resume/history/activity queries. The runner uses the existing guest report and teardown checks. The final read-only SQLite check matches the actual WS chat cursor order, original Ask choices, and each returned security log record. The guest uses its existing native probes and controlled HTTPS fixture. Original OpenClaw acceptance runs through the designated chat. [Interface and evidence contract](../../docs/middleware/smoke/technical.md#脚本化前端接口)

The original Ask workflow runs File.Open, Process.Execute, Network.Connect and Network.Send with both Allow and Deny through the same WS session and receipt checks. Send exercises sendto/sendmsg, socket write/writev and single-message sendmmsg; message counts remain distinct from byte counts. It reuses the existing subject/family probes and compares actual effects and the matching native return in SQLite. Rules cover resource directories or the actual listening endpoint, without selecting an actor PID or adding a test Allow. These are native guest protection tests; they do not establish stock-Agent attribution, real GLM or fixed-feedback delivery.

## Stock OpenClaw with real GLM

The fixed real-GLM entry is [openclaw-glm/prepare.py](openclaw-glm/prepare.py),
with [scope and workflow](openclaw-glm/README.md). It consolidates the former
local file-only generator and R96 case extension into the existing execution
chain. File, process and network effects have separate oracles; phantom states
are checked per original attempt. Preparation is not stock/model execution.

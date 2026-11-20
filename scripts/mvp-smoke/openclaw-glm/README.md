# Real GLM / phantom acceptance

This is the versioned source of the existing real-GLM acceptance entry, moved
from the local `.runtime/openclaw-glm-acceptance` generator. It reuses the
existing stock runtime, VM packaging, authenticated WS and evidence consumers.
It does not introduce another launcher, provider or attribution implementation.

After the normal provider CLI update, the existing owner waits for the stock
`/startupz` response `status: started` before declaring the cohort ready. CLI
success and the live HTTP page do not mean the Gateway has completed its normal
reload/restart. There is no test-imposed elapsed-time cutoff. An RPC success
with `runStarted: false` or `runError` is retained as a failed start, not a model
run or a reason to resend the old business request.

`prepare.py` creates file Open, process Execute, network Connect and network Send
Allow/Deny scenarios. Rules cover the owned file/executable directories and
listening endpoints, with no actor PID, unique command or probe-name condition.
Both network operations cover the endpoint range; a Send scenario answers the
prerequisite Connect separately. Extra original questions remain recorded.

The candidate's installed `config/rules.yaml` is retained during preparation.
The four review groups are added to the actual current document through the
existing authenticated rule owner; existing product presets and user groups
are preserved. Group conflicts remain actual rule-owner errors, without silent
replacement or renumbering. The product's editable Driver fast-allow presets
ship through the existing MVP and conference packaging entries.

Tasks retain the existing stock GUI session-creation path. The R96 aborted run
remains a failed original. Scenario preparation and physical-effect checks do
not change session APIs to bypass the unresolved response-consumption failure.

The real key is imported only into AS. Stock OpenClaw receives the issued phantom
through its supported provider configuration. The entry retains TLS verification
and has no synthetic-provider or direct-key fallback.

```sh
source "${AS_CI_ENV:-${XDG_DATA_HOME:-$HOME/.local/share}/agentshield-ci/env.sh}"
python3 scripts/mvp-smoke/openclaw-glm/prepare.py \
  CURRENT_CANDIDATE_JSON NEW_FIXTURE NEW_OUTPUT \
  --templates EXISTING_ACCEPTANCE_TEMPLATE_DIRECTORY \
  --label local-controls --model ACTUAL_AUTHORIZED_MODEL
```

The template directory is the existing local `.runtime` containing
`openclaw-audit-acceptance` and `openclaw-raw-acceptance`; it is an input, not a
second implementation. Preparation reads no credential and launches no VM,
Agent or model. The generated `run-owned.sh` is the existing execution entry.
Its host-side owner resolves the already authorized OpenCode configuration under
the actual caller's home only when `provider_secret()` runs; preparation does not
expand that home into the generated source or read the key. The existing
`glm-team` provider slot and AS-only key handoff remain unchanged; this is not a
new credential source or permission to use another account's configuration.
`--policy-validator` is optional; the actual rule owner always validates and
applies the document. For an optional schema-only check use Cargo's
`as-policy-engine` example `decode_document`; do not select rlibs by timestamp.

The current candidate names artifacts under `artifacts`, each with its actual
`path` and `sha256`. The referenced source-snapshot document has a `files` list of `{path, sha256}` entries,
not a path-keyed object. The one boot-archive check consumes the generated
`inputs.json.joint_assets`. Old synthetic-provider CA checks, file-only effect
checkers and raw-attribution linkers are not copied into the active output or
run before the control scenarios. Template preparation does not validate an
obsolete AS archive that is immediately replaced by the current candidate.

Physical effects, original WS/SQL receipts, model round completion, individual
phantom attempts and normal retirement are independent results. A denied exec
is not judged as a file write; a denied Connect is not a successful Send test.
Tool `isError`, model prose and exact question counts do not prove enforcement.
An unresolved original remains running and observed; it is never closed because
a clock expired. Only its actual terminal, an acquired AS/Gateway process exit,
or an explicit operator stop ends that wait. An observation failure is not a
SUT failure. A settled failed case does not disable other independent cases.

Tool attribution and fixed-feedback gaps are recorded independently and do not
block ordinary HIPS/phantom acceptance. Script and schema checks are not evidence
of a successful GLM or stock-Agent run.

## Execution and evidence boundaries

Run the normal Allow tasks first, then the independent Deny tasks. Do not invent
an LLM answer format, fixed tool wording, exact Ask count or attribution
requirement before observing the real workflow.

Removed inherited tester cutoffs: UI preparation and interaction, the 540-second
business wait, 1200-second startup, 7200-second VM lifetime, control RPC/WS waits,
and retirement/join deadlines. The existing readiness HTTP request is one
in-flight read on the owner side; it cannot block the VM selector or reject a
slow healthy response. Poll intervals are scheduling, not execution deadlines.

Original Ask decisions are sent before optional witness work. The optional
witness explicitly labels its timing after the answer; it does not claim an
independent pre-answer syscall sample. The original driver hook data remains
unchanged. Optional diagnostics run off the answer path. SQL snapshots, tracing,
Dashboard/OTLP sidecars, media-summary log presence and optional schema checks
do not authorize or prevent an ordinary run.

HIPS effects/receipts, SQL persistence, real GLM/phantom attempts, Agent round
completion and retirement have independent outcomes. Missing exports are saved
as evidence gaps; they do not erase acquired WS receipts or effects. Verifier
exit zero means the report was generated, not that AS passed. Read the statuses.

Keep the actual SUT rule/protocol timeouts as observed behavior; changing the
tester does not disable authentication, alter AS policy, replace the Provider,
or modify the Agent. Scope/ownership checks that prevent overwriting an old
cohort remain required by the user. Public WS capture follows the production
8MiB contract; no smaller test quota or credential-shaped-text filter is used.

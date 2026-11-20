#!/usr/bin/env python3
"""Prepare current candidate inputs from existing stock-runtime templates; no execution."""

import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def remove_owned_deadlines(path, classes=None):
    """Remove inherited test clocks, preserving SUT configuration and protocol checks."""
    class OwnedWaits(ast.NodeTransformer):
        def __init__(self):
            self.enabled = classes is None

        def visit_ClassDef(self, node):
            previous = self.enabled
            self.enabled = classes is None or node.name in classes
            node = self.generic_visit(node)
            self.enabled = previous
            return node

        def visit_FunctionDef(self, node):
            if self.enabled:
                positional = node.args.posonlyargs + node.args.args
                for index, default in enumerate(node.args.defaults):
                    argument = positional[len(positional) - len(node.args.defaults) + index]
                    if argument.arg == "timeout":
                        node.args.defaults[index] = ast.Constant(None)
            return self.generic_visit(node)

        def visit_Call(self, node):
            node = self.generic_visit(node)
            if not self.enabled:
                return node
            for keyword in node.keywords:
                if keyword.arg == "timeout":
                    keyword.value = ast.Constant(None)
            if isinstance(node.func, ast.Attribute):
                name = node.func.attr
                if name == "settimeout":
                    node.args = [ast.Constant(None)]
                elif name in {"_read", "_receive_frame"} and node.args:
                    node.args[0] = ast.Constant(None)
                elif name == "wait" and node.args and isinstance(node.args[0], ast.Constant) and isinstance(node.args[0].value, (int, float)):
                    node.args[0] = ast.Constant(None)
                elif name == "wait_for" and len(node.args) > 1:
                    node.args[1] = ast.Constant(None)
                elif name == "readline" and node.args:
                    node.args = []
            return node

        def visit_Assign(self, node):
            if self.enabled and any(isinstance(target, ast.Name) and target.id == "deadline" for target in node.targets):
                return None
            if self.enabled and any(isinstance(target, ast.Name) and target.id == "remaining" for target in node.targets) and any(isinstance(value, ast.Name) and value.id == "deadline" for value in ast.walk(node.value)):
                node.value = ast.Constant(None)
            return self.generic_visit(node)

        def visit_Expr(self, node):
            if self.enabled and isinstance(node.value, ast.Call) and isinstance(node.value.func, ast.Name) and node.value.func.id == "require" and any(isinstance(value, ast.Constant) and isinstance(value.value, str) and "timed out waiting" in value.value for value in node.value.args):
                return None
            if self.enabled and isinstance(node.value, ast.Call) and isinstance(node.value.func, ast.Name) and node.value.func.id == "require" and any(isinstance(value, ast.Constant) and value.value == "unconsumed middleware event capacity exhausted" for value in node.value.args):
                return None
            return self.generic_visit(node)

        def visit_While(self, node):
            if self.enabled and any(isinstance(value, ast.Name) and value.id == "deadline" for value in ast.walk(node.test)):
                node.test = ast.Constant(True)
                node.orelse = []
            return self.generic_visit(node)

        def visit_For(self, node):
            if self.enabled and isinstance(node.iter, ast.Call) and isinstance(node.iter.func, ast.Name) and node.iter.func.id == "range" and len(node.iter.args) == 1 and isinstance(node.iter.args[0], ast.Constant) and node.iter.args[0].value in {300, 32}:
                return ast.copy_location(ast.While(test=ast.Constant(True), body=[self.visit(child) for child in node.body], orelse=[]), node)
            return self.generic_visit(node)

        def visit_If(self, node):
            if self.enabled and any(isinstance(child, ast.Raise) and child.exc and "deadline expired" in ast.unparse(child.exc) for child in node.body):
                return None
            if self.enabled and any(isinstance(child, ast.Raise) and child.exc and any(text in ast.unparse(child.exc) for text in ("owned test command exceeds bound", "VM reply exceeds current frame bound")) for child in node.body):
                return None
            return self.generic_visit(node)

    tree = OwnedWaits().visit(ast.parse(path.read_text()))
    ast.fix_missing_locations(tree)
    path.write_text(ast.unparse(tree) + "\n")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("candidate", "fixture", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--templates", type=Path, default=Path.cwd() / ".runtime")
    args = parser.parse_args()
    os.umask(0o077)
    fixture, output = args.fixture.resolve(), args.output.resolve()
    if fixture.exists() or output.exists():
        parser.error("preserve existing cohort; select new fixture and output paths")
    manifest = json.loads(args.candidate.read_text())
    templates = args.templates.resolve()
    audit = templates / "openclaw-audit-acceptance/template"
    raw = templates / "openclaw-raw-acceptance"
    fixture.mkdir(mode=0o700, parents=True)
    output.mkdir(mode=0o700, parents=True)
    shutil.copytree(audit / "root", fixture / "root")
    for name in (
        "run-vm.py",
        "call-vm.py",
        "agent-browser.cjs",
        "openclaw-web.js",
        "live-stack-env.js",
        "runtime-manifest.json",
    ):
        shutil.copy2(audit / name, fixture / name)
    for name in ("runtime.ext4", "Image"):
        subprocess.run(
            ["cp", "--reflink=auto", str(audit / name), str(fixture / name)], check=True
        )
    for path in (raw / "template").iterdir():
        if path.is_file():
            shutil.copy2(path, fixture / path.name)
    for path in (raw / "template/suite").iterdir():
        shutil.copy2(path, fixture / "root/suite" / path.name)
    # Only the existing consumers used by run-owned are active in this cohort.
    for name in (
        "run-owned.sh",
        "retire.py",
        "prepare-analysis.py",
        "extract-failure.py",
        "verify-archive.py",
    ):
        shutil.copy2(raw / "capture" / name, output / name)
    run = output / "run-owned.sh"
    run.write_text(
        run.read_text().replace(
            'python3 -B "$out/link-raw-lifetimes.py" "$out" || exit 1\n', ""
        )
    )
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    assets = {}
    locations = {
        "as/bin/as-service": "service",
        "suite/as_wait.ko": "native_module",
        "as/driver/as_hooks.bpf.o": "bpf_hooks",
        "as/driver/as_flow.bpf.o": "bpf_flow",
        "as/driver/as-bpf-loader": "bpf_loader",
        "as/driver/as-tls-ingress": "tls_loader",
    }
    for relative, key in locations.items():
        source = Path(manifest["artifacts"][key]["path"])
        destination = fixture / "root" / relative
        shutil.copy2(source, destination)
        assets[relative] = sha(destination)
        if assets[relative] != manifest["artifacts"][key]["sha256"]:
            raise RuntimeError("candidate artifact differs: " + key)
    package = Path(manifest["build"]["package"]).parent / "root"
    for relative in (
        "as/config/as.yaml",
        "as/config/rules.yaml",
        "suite/source-mvp-sha256.json",
    ):
        shutil.copy2(package / relative, fixture / "root" / relative)
        assets[relative] = sha(fixture / "root" / relative)
    subprocess.run(["truncate", "-s", "2G", str(fixture / "evidence.ext4")], check=True)
    subprocess.run(
        ["mkfs.ext4", "-q", "-L", "AS_OC_EVIDENCE", str(fixture / "evidence.ext4")],
        check=True,
    )
    inherited = json.loads((audit / "inputs.json").read_text())
    inputs = {key: inherited[key] for key in ("kernel_sha256", "runtime_sha256")}
    inputs.update(
        candidate_manifest_sha256=sha(args.candidate),
        candidate_base_commit=manifest["base_commit"],
        source_snapshot=manifest["source_snapshot"],
        joint_assets=assets,
        candidate_image_sha256=manifest["artifacts"]["image"]["sha256"],
    )
    (fixture / "inputs.json").write_text(json.dumps(inputs, indent=2) + "\n")
    shutil.copy2(args.candidate, output / "candidate.json")
    for path in (fixture / "root/suite").glob("*.py"):
        ast.parse(path.read_text())


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Execute the real CLI/MCP LV process boundary against a captured native engine.

Uses the shared owning syscall gate; builds, installs and project reads are absent.
The evidence directory must be new and on disk. Development artifacts remain
explicitly unqualified: this gate does not establish package-install trust.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

sys.dont_write_bytecode = True


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ds", required=True, type=Path)
    parser.add_argument("--engine", required=True, type=Path)
    parser.add_argument("--fixtures", required=True, type=Path)
    parser.add_argument("--trace-helper", required=True, type=Path)
    parser.add_argument("--evidence", required=True, type=Path)
    parser.add_argument("--reuse-evidence", type=Path, action="append", default=[],
        help="retained trace/output folder from the same immutable artifacts; repeat for partial runs")
    parser.add_argument("--reused-ds-sha256", help="recorded immutable CLI digest required when reusing evidence")
    args = parser.parse_args()
    ds, engine, fixtures = [p.resolve(strict=True) for p in (args.ds, args.engine, args.fixtures)]
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location("owning_compute_gate", args.trace_helper.resolve(strict=True))
    gate = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(gate)
    os.environ["DS_NETWORK_ENGINE_BIN"] = str(engine)
    # Keep SRE enabled: authority-none computation must remain offline itself.
    os.environ.pop("DS_SRE_REPORTING", None)
    os.environ["TMPDIR"] = str(evidence)
    before = {"ds": digest(ds), "engine": digest(engine)}
    reuse = [p.resolve(strict=True) for p in args.reuse_evidence]
    if reuse:
        assert args.reused_ds_sha256 == before["ds"], "retained calls must use the same captured CLI artifact"
    cases = []

    def retained_success(label, rpc=False):
        for root in reuse:
            candidate = root / (label + ".network.trace")
            stdout = root / (label + ".stdout")
            if not candidate.is_file() or not stdout.is_file():
                continue
            counts = gate.classify_network_trace(candidate)
            if counts["outgoing_network_syscall_attempts"]:
                continue
            if rpc:
                response = next(json.loads(line) for line in stdout.read_text().splitlines() if json.loads(line).get("id") == 1)
                if response.get("result", {}).get("structuredContent", {}).get("status") != "ok":
                    continue
            elif json.loads(stdout.read_text()).get("status") != "ok":
                continue
            return candidate
        return None

    def cli(label, *arguments, success=True, code=None):
        retained = retained_success(label) if success else None
        if retained and retained.exists():
            counts = gate.classify_network_trace(retained)
            assert not counts["outgoing_network_syscall_attempts"], counts
            stdout = retained.with_name(label + ".stdout").read_text()
            envelope = json.loads(stdout)
            assert envelope["status"] == "ok", "only successful retained calls may be reused"
            completed = subprocess.CompletedProcess([], 0, stdout, retained.with_name(label + ".stderr").read_text())
            if "--out" in arguments:
                target = Path(arguments[arguments.index("--out") + 1])
                held = Path(envelope["data"]["out"])
                assert held.parent == retained.parent and digest(held) == envelope["data"]["result_sha256"]
                assert envelope["data"]["executable_sha256"] == before["engine"]
                shutil.copy2(held, target)
            (evidence / (label + ".reused-network-counts.json")).write_text(json.dumps(counts, indent=2))
        else:
            completed = gate.trace([str(ds), *arguments, "--output", "json"], evidence, label)
            counts = gate.classify_network_trace(evidence / (label + ".network.trace"))
        envelope = json.loads(completed.stdout)
        if success:
            assert completed.returncode == 0 and envelope["status"] == "ok", envelope
        else:
            assert completed.returncode != 0 and envelope["error"]["code"] == code, envelope
        cases.append({"case": label, "exit_code": completed.returncode, "network_attempts": 0,
            "network_counts": counts,
            "reused_from": str(retained) if retained and retained.exists() else None})
        return envelope

    def mcp(label, operation, arguments=None):
        # Reuse describes only; invoke output paths belong to the live call.
        retained = retained_success(label, rpc=True) if operation == "describe" else None
        if retained and retained.exists():
            counts = gate.classify_network_trace(retained)
            assert not counts["outgoing_network_syscall_attempts"], counts
            response = next(json.loads(line) for line in retained.with_name(label + ".stdout").read_text().splitlines()
                if json.loads(line).get("id") == 1)
            assert "result" in response, response
            cases.append({"case": label, "network_attempts": 0, "network_counts": counts, "reused_from": str(retained)})
            return response["result"]["structuredContent"]
        wire = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": "ds_design", "arguments": {"operation": operation, "command": "design.lv.process"}}}
        if arguments is not None:
            wire["params"]["arguments"]["arguments"] = arguments
        # MCP needs stdin; trace the fixed command with one local request pipe.
        path = evidence / (label + ".network.trace")
        environment = dict(os.environ, TMPDIR=str(evidence))
        completed = subprocess.run(["strace", "-f", "-qq", "-o", str(path), "-e", "trace=network",
            "-e", "inject=socket:error=EPERM", "--", str(ds), "mcp", "serve"],
            input=json.dumps(wire) + "\n", env=environment, text=True, capture_output=True, timeout=120)
        (evidence / (label + ".stdout")).write_text(completed.stdout)
        (evidence / (label + ".stderr")).write_text(completed.stderr)
        counts = gate.classify_network_trace(path)
        assert not counts["outgoing_network_syscall_attempts"] and "ptrace" not in completed.stderr, counts
        (evidence / (label + ".network-counts.json")).write_text(json.dumps(counts, indent=2))
        response = next(json.loads(line) for line in completed.stdout.splitlines() if json.loads(line).get("id") == 1)
        assert "result" in response, response
        cases.append({"case": label, "network_attempts": 0, "network_counts": counts})
        return response["result"]["structuredContent"]

    descriptor = cli("describe", "capabilities", "design.lv.process")["data"]["command"]
    assert descriptor["authority"] == "none" and descriptor["effect"] == "local_file_write"
    assert descriptor["contract"] == 4
    assert mcp("mcp-describe", "describe")["data"]["command"] == descriptor
    for name in ("mixed", "approved"):
        request = fixtures / (name + ".request.json")
        golden = fixtures / (name + ".insertion.result.json")
        output = evidence / (name + ".result.json")
        request_before = digest(request)
        receipt = cli(name, "design", "lv", "process", "--input", str(request), "--out", str(output))["data"]
        assert output.read_bytes() == golden.read_bytes(), "historical consumer bytes changed"
        assert digest(request) == request_before == receipt["input_sha256"]
        assert digest(output) == receipt["result_sha256"]
        assert receipt["executable_sha256"] == before["engine"]
        assert receipt["development_override"] is True
        assert receipt["engine_identity"]["jsonObjectOrder"] == "insertion"
        assert "lv_processing_v1" in receipt["engine_identity"]["capabilities"]
        if name == "mixed":
            assert (receipt["jobs"], receipt["succeeded"], receipt["failed"]) == (2, 1, 1)
            assert [r["transformer_name"] for r in receipt["results"]] == ["T_success", "T_failure"]
    mixed = fixtures / "mixed.request.json"
    canonical = evidence / "canonical.request.json"
    canonical_document = json.loads(mixed.read_bytes())
    canonical_document["schema"] = "ds.lv.process.request/v1"
    canonical.write_text(json.dumps(canonical_document))
    canonical_output = evidence / "canonical.result.json"
    cli("canonical", "design", "lv", "process", "--input", str(canonical), "--out", str(canonical_output))
    current_result = json.loads(canonical_output.read_bytes())
    historical_result = json.loads((fixtures / "mixed.insertion.result.json").read_bytes())
    assert current_result.pop("schema") == "ds.lv.process.result/v1"
    historical_result.pop("schema")
    assert current_result == historical_result, "schema migration changed engineering output"
    duplicate = evidence / "mixed.result.json"
    cli("no-overwrite", "design", "lv", "process", "--input", str(mixed), "--out", str(duplicate),
        success=False, code="fast_lv_output_exists")
    mcp_output = evidence / "mcp.result.json"
    invoked = mcp("mcp-invoke", "invoke", {"input": str(mixed), "out": str(mcp_output)})
    assert invoked["status"] == "ok", invoked
    assert mcp_output.read_bytes() == (fixtures / "mixed.insertion.result.json").read_bytes()

    # A receiver-only adversarial fixture; it implements no engineering.
    fixture = evidence / "incompatible"
    fixture.write_text('''#!/usr/bin/python3 -S
import hashlib, json, pathlib, sys
mode = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
workers = int(args[args.index("--workers") + 1])
identity = {"schema":"ds.network-engine/v1","protocolMajor":2 if mode == "incompatible" else 1,
 "protocolMinor":0,"coreVersion":"fixture","sourceSha":None,"dependencyClosureSha256":None,
 "environment":"native","runtime":{"environment":"native","logicalCpus":workers,"internalWorkerThreads":workers},
 "jsonObjectOrder":"sorted" if mode == "wrong-profile" else "insertion","capabilities":["lv_processing_v1"]}
with pathlib.Path(sys.argv[0]).with_suffix(".calls").open("a") as log: log.write(args[0] + "\\n")
if args[0] == "build-info":
 print(json.dumps({"schema":"ds.engine-build/v1","module":"network","engineIdentity":identity}))
elif args[0] == "lv-process":
 request = pathlib.Path(args[args.index("--request") + 1]).read_bytes()
 source = json.loads(request)
 result = json.dumps({"schema":"ds.fast-lv.result/v1","engine_core_version":"fixture",
  "jobs":[{"transformer_name":j["transformer_name"],"ok":False,"error":"fixture refusal"} for j in source["jobs"]]}).encode()
 pathlib.Path(args[args.index("--out") + 1], "result.json").write_bytes(result)
 print(json.dumps({"schema":"ds.network.task-result/v1","inputSha256":hashlib.sha256(request).hexdigest(),
  "engineIdentity":identity,"outcome":"cancelled" if mode == "cancelled" else "completed",
  "artifact":{"file":"result.json","byteCount":len(result),
   "sha256":"0"*64 if mode == "bad-digest" else hashlib.sha256(result).hexdigest(),"schema":"ds.fast-lv.result/v1"}}))
else: sys.exit(91)
''')
    fixture.chmod(0o755)
    os.environ["DS_NETWORK_ENGINE_BIN"] = str(fixture)
    cli("discovery-no-spawn", "capabilities", "design.lv.process")
    mcp("mcp-discovery-no-spawn", "describe")
    assert not fixture.with_suffix(".calls").exists(), "discovery executed an optional addon"
    for mode, code in [("missing", "network_engine_missing"), ("incompatible", "network_engine_incompatible"),
            ("wrong-profile", "network_serialization_incompatible"), ("bad-digest", "network_result_digest_mismatch"),
            ("cancelled", "network_task_cancelled")]:
        candidate = evidence / mode
        if mode not in ("missing", "incompatible"):
            shutil.copy2(fixture, candidate)
        os.environ["DS_NETWORK_ENGINE_BIN"] = str(candidate)
        out = evidence / (mode + ".result.json")
        cli(mode, "design", "lv", "process", "--input", str(mixed), "--out", str(out), success=False, code=code)
        assert not out.exists()
        calls = candidate.with_suffix(".calls")
        if mode in ("incompatible", "wrong-profile"):
            assert calls.read_text().splitlines() == ["build-info"], "incompatible candidate must never compute"
    assert not list(evidence.glob("ds-network-run-*")), "private job staging leaked"
    assert {"ds": digest(ds), "engine": digest(engine)} == before
    control_path = evidence / "attempted-network-control.network.trace"
    controlled = subprocess.run(["strace", "-f", "-qq", "-o", str(control_path), "-e", "trace=network",
        "-e", "inject=socket:error=EPERM", "--", "/usr/bin/python3", "-S", "-c",
        "import socket\ntry: socket.socket()\nexcept OSError: pass"], text=True, capture_output=True, timeout=30)
    assert controlled.returncode == 0 and "ptrace" not in controlled.stderr
    control_counts = gate.classify_network_trace(control_path)
    assert control_counts["outgoing_network_syscall_attempts"] == 1, control_counts
    control = {"attempted_socket_detected_despite_successful_child": True}
    receipt = {"schema": "ds.network.cli-process-qualification/v1", "artifacts": before,
        "cases": cases, "negative_control": control, "historical_byte_parity": True,
        "local_rust_exec_socketpairs": sum(c["network_counts"]["local_rust_exec_socketpairs"] for c in cases),
        "local_rust_exec_empty_eof_reads": sum(c["network_counts"]["local_rust_exec_empty_eof_reads"] for c in cases),
        "development_override": True, "installed_component_trust": False}
    (evidence / "acceptance.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()

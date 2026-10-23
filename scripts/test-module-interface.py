#!/usr/bin/env python3
"""Qualify a built Linux ds addon interface without Cargo or project access."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class ModuleInterface(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="module-interface-", dir=SCRATCH)
        self.root = Path(self.scratch.name)
        self.env = dict(os.environ, DS_SRE_REPORTING="off")
        self.engines = {}
        for component, override, identity in [
            ("solar", "DS_SOLAR_ENGINE_BIN", {"name": "ds-solar-engine", "version": "0.1.0", "schemas": ["ds-solar.prepared-calculation/v1"]}),
            ("reporter", "DS_REPORT_BIN", {"schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report", "schemas": ["ds.report.task/v1"]}),
            ("network", "DS_NETWORK_ENGINE_BIN", {"schema":"ds.engine-build/v1", "module":"network", "engineIdentity":{"schema":"ds.network-engine/v1", "protocolMajor":1, "protocolMinor":0, "coreVersion":"1.4.0", "sourceSha":None, "dependencyClosureSha256":None, "environment":"native", "runtime":{"environment":"native", "logicalCpus":2, "internalWorkerThreads":1}, "jsonObjectOrder":"sorted", "capabilities":["lv_processing_v1", "future_operation_v2"]}}),
        ]:
            path = self.root / ("ds-network" if component == "network" else component)
            path.write_text("#!/usr/bin/env python3\nimport json, pathlib, sys\np = pathlib.Path(__file__)\nwith p.with_suffix('.calls').open('a') as f: f.write(json.dumps(sys.argv[1:]) + '\\n')\nif sys.argv[1:] != ['build-info']: sys.exit(91)\nprint(p.with_suffix('.identity').read_text())\n")
            path.chmod(0o755)
            path.with_suffix(".identity").write_text(json.dumps(identity))
            self.env[override] = str(path)
            self.engines[component] = path

    def tearDown(self):
        self.scratch.cleanup()

    def cli(self, *args):
        result = subprocess.run([str(DS), *args, "--output", "json"], env=self.env, text=True, capture_output=True, timeout=30)
        self.assertTrue(result.stdout, result.stderr)
        return json.loads(result.stdout)

    def mcp(self, command, arguments=None, operation="invoke"):
        inputs = {"operation": operation, "command": command}
        if operation == "invoke":
            inputs["arguments"] = arguments or {}
        request = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "ds_design", "arguments": inputs}}
        result = subprocess.run([str(DS), "mcp", "serve"], env=self.env, input=json.dumps(request) + "\n", text=True, capture_output=True, timeout=30)
        response = next(json.loads(line) for line in result.stdout.splitlines() if json.loads(line).get("id") == 1)
        self.assertIn("result", response, response)
        return response["result"]["structuredContent"]

    def calls(self, component):
        path = self.engines[component].with_suffix(".calls")
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_discovery_starts_no_engine_and_explicit_inventory_probes_once(self):
        descriptor = self.cli("capabilities", "server.modules")["data"]["command"]
        self.assertEqual(descriptor["authority"], "none")
        self.assertEqual(self.mcp("server.modules", operation="describe")["data"]["command"], descriptor)
        self.assertEqual([self.calls(c) for c in self.engines], [[], [], []])
        direct = self.cli("server", "modules")
        self.assertEqual(direct["data"]["schema"], "ds.module-inventory/v1")
        self.assertEqual([(r["component"], r["status"]) for r in direct["data"]["modules"]], [("solar", "available"), ("reporter", "available"), ("network", "available")])
        self.assertEqual([self.calls(c) for c in self.engines], [[["build-info"]], [["build-info"]], [["build-info"]]])
        self.assertEqual(self.mcp("server.modules")["data"], direct["data"])
        self.assertEqual([len(self.calls(c)) for c in self.engines], [2, 2, 2])

    def test_missing_override_never_falls_back_and_wrong_identity_is_refused(self):
        self.env["DS_SOLAR_ENGINE_BIN"] = str(self.root / "missing")
        self.engines["reporter"].with_suffix(".identity").write_text(json.dumps({"name": "ds-solar-engine", "schemas": []}))
        rows = self.cli("server", "modules")["data"]["modules"]
        self.assertEqual([(r["component"], r["status"]) for r in rows], [("solar", "missing"), ("reporter", "refused"), ("network", "available")])
        self.assertEqual(rows[0]["error"]["code"], "solar_engine_missing")
        self.assertEqual(rows[1]["error"]["code"], "callee_contract_mismatch")
        self.assertEqual(self.calls("solar"), [])
        self.assertEqual(self.calls("reporter"), [["build-info"]])

    def test_network_identity_and_missing_engine_are_optional_bounded_rows(self):
        rows = self.cli("server", "modules")["data"]["modules"]
        network = rows[2]
        self.assertEqual(network["executable"], "ds-network")
        self.assertEqual(network["identity"]["engineIdentity"]["coreVersion"], "1.4.0")
        self.assertEqual(network["capabilities"], ["lv_processing_v1", "future_operation_v2"])
        path = self.engines["network"].with_suffix(".identity")
        original = json.loads(path.read_text())
        # Owner 22 Oct 2026: another protocolMajor is provenance, not a refusal;
        # a protocolMajor of the wrong type still is.
        for field, invalid in [("protocolMajor", "1"), ("environment", "wasm"), ("sourceSha", "a" * 40), ("capabilities", ["x" * 257])]:
            changed = json.loads(json.dumps(original))
            changed["engineIdentity"][field] = invalid
            path.write_text(json.dumps(changed))
            rows = self.cli("server", "modules")["data"]["modules"]
            self.assertEqual(rows[2]["status"], "refused", field)
            self.assertEqual(rows[2]["error"]["code"], "callee_contract_mismatch")
            self.assertEqual(rows[0]["status"], "available")
        self.env["DS_NETWORK_ENGINE_BIN"] = str(self.root / "missing")
        rows = self.cli("server", "modules")["data"]["modules"]
        self.assertEqual(rows[2]["status"], "missing")
        self.assertEqual(rows[2]["error"]["code"], "network_engine_missing")
        self.assertEqual(len(self.calls("network")), 5)

    def test_consuming_network_inspects_only_its_selected_owner(self):
        direct = self.cli("server", "modules", "--component", "network")
        self.assertEqual(direct["status"], "ok", direct)
        self.assertEqual([(r["component"], r["status"]) for r in direct["data"]["modules"]], [("network", "available")])
        self.assertEqual([self.calls(c) for c in self.engines], [[], [], [["build-info"]]])
        self.assertEqual(self.mcp("server.modules", {"component":"network"})["data"], direct["data"])
        self.assertEqual([len(self.calls(c)) for c in self.engines], [0, 0, 2])

    def test_network_catalog_and_update_inspection_never_start_an_engine(self):
        release = {"schema":"ds.module-release/v1", "component":"network", "channel":"canary", "product_id":"rw.datasolutions.module.network.canary", "version":"1.4.0-r1", "protocol":"ds.network-engine/v1", "released_at":"2026-10-14T00:00:00Z", "status":"active", "assets":[{"platform":"linux", "arch":"x86_64", "kind":"module-executable", "protocol":"ds.network-engine/v1", "url":"https://storage.googleapis.com/releases/modules/network/canary/1.4.0-r1/ds-network", "size_bytes":10, "sha256":"a"*64, "source_sha":"b"*40, "built_at":"2026-10-13T00:00:00Z", "source_revisions":{"ds-network":"b"*40}, "engines":[{"engine":"ds-network", "release":"ds-network@1.4.0+"+"b"*40, "manifest_sha256":"c"*64}]}]}
        path = self.root / "network-release.json"
        path.write_text(json.dumps(release))
        direct = self.cli("server", "module-release", "--component", "network", "--channel", "canary", "--file", str(path))
        self.assertEqual(direct["status"], "ok", direct)
        self.assertEqual(self.mcp("server.module-release", {"component":"network", "channel":"canary", "file":str(path)})["data"], direct["data"])
        identity = json.loads(self.engines["network"].with_suffix(".identity").read_text())
        path.write_text(json.dumps({"release":release, "installed":identity, "platform":"linux"}))
        inspected = self.cli("server", "update-inspect", "--component", "network", "--channel", "canary", "--file", str(path))
        self.assertEqual(inspected["status"], "ok", inspected)
        self.assertEqual(inspected["data"]["installed"], {"version":"1.4.0", "buildNumber":None, "releaseDate":None})
        self.assertEqual(inspected["data"]["published"], {"version":"1.4.0", "buildNumber":1, "releaseDate":"2026-10-14T00:00:00Z"})
        self.assertIsNone(inspected["data"]["update"])
        self.assertEqual([self.calls(c) for c in self.engines], [[], [], []])

    def test_bounded_catalog_admission_is_cli_mcp_identical_without_engine_reads(self):
        manifest = {"schema": "ds.module-release/v1", "component": "solar", "channel": "canary", "product_id": "rw.datasolutions.module.solar.canary", "version": "0.1.0-r2", "protocol": "ds.module/v1", "released_at": "2026-10-13T08:00:00Z", "status": "blocked", "assets": [{"platform": "linux", "arch": "x86_64", "kind": "module-executable", "protocol": "ds.module/v1", "url": "https://storage.googleapis.com/dedicated-public-releases/modules/solar/canary/0.1.0-r1/solar", "size_bytes": 10, "sha256": "a" * 64, "source_sha": "b" * 40, "built_at": "2026-10-12T08:00:00Z", "source_revisions": {"ds-solar": "b" * 40}, "engines": [{"engine": "ds-solar", "release": "ds-solar@0.1.0+" + "b" * 40, "manifest_sha256": "d" * 64}]}]}
        path = self.root / "release.json"
        path.write_text(json.dumps({"release": manifest}))
        direct = self.cli("server", "module-release", "--component", "solar", "--channel", "canary", "--file", str(path))
        self.assertEqual(direct["status"], "ok", direct)
        self.assertEqual(direct["data"]["status"], "blocked")
        self.assertEqual(self.mcp("server.module-release", {"component": "solar", "channel": "canary", "file": str(path)})["data"], direct["data"])
        refused = self.cli("server", "module-release", "--component", "reporter", "--channel", "canary", "--file", str(path))
        self.assertEqual(refused["error"]["code"], "module_release_invalid")
        path.write_text(json.dumps({"release": None}))
        self.assertIsNone(self.cli("server", "module-release", "--component", "solar", "--channel", "canary", "--file", str(path))["data"])
        path.write_bytes(b" " * (2 * 1024 * 1024 + 1))
        self.assertEqual(self.cli("server", "module-release", "--component", "solar", "--channel", "canary", "--file", str(path))["error"]["code"], "module_release_invalid")
        self.assertEqual([self.calls(c) for c in self.engines], [[], [], []])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ds", required=True, type=Path)
    parser.add_argument("--scratch", required=True, type=Path, help="existing directory on disk, outside tmpfs")
    args = parser.parse_args()
    DS = args.ds.resolve(strict=True)
    SCRATCH = args.scratch.resolve(strict=True)
    unittest.main(argv=[__file__], verbosity=2)

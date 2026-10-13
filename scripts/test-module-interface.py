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
        ]:
            path = self.root / component
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
        self.assertEqual([self.calls(c) for c in self.engines], [[], []])
        direct = self.cli("server", "modules")
        self.assertEqual(direct["data"]["schema"], "ds.module-inventory/v1")
        self.assertEqual([(r["component"], r["status"]) for r in direct["data"]["modules"]], [("solar", "available"), ("reporter", "available")])
        self.assertEqual([self.calls(c) for c in self.engines], [[["build-info"]], [["build-info"]]])
        self.assertEqual(self.mcp("server.modules")["data"], direct["data"])
        self.assertEqual([len(self.calls(c)) for c in self.engines], [2, 2])

    def test_missing_override_never_falls_back_and_wrong_identity_is_refused(self):
        self.env["DS_SOLAR_ENGINE_BIN"] = str(self.root / "missing")
        self.engines["reporter"].with_suffix(".identity").write_text(json.dumps({"name": "ds-solar-engine", "schemas": []}))
        rows = self.cli("server", "modules")["data"]["modules"]
        self.assertEqual([(r["component"], r["status"]) for r in rows], [("solar", "missing"), ("reporter", "refused")])
        self.assertEqual(rows[0]["error"]["code"], "solar_engine_missing")
        self.assertEqual(rows[1]["error"]["code"], "callee_contract_mismatch")
        self.assertEqual(self.calls("solar"), [])
        self.assertEqual(self.calls("reporter"), [["build-info"]])

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
        self.assertEqual([self.calls(c) for c in self.engines], [[], []])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ds", required=True, type=Path)
    parser.add_argument("--scratch", required=True, type=Path, help="existing directory on disk, outside tmpfs")
    args = parser.parse_args()
    DS = args.ds.resolve(strict=True)
    SCRATCH = args.scratch.resolve(strict=True)
    unittest.main(argv=[__file__], verbosity=2)

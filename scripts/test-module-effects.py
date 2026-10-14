#!/usr/bin/env python3
"""Qualify built native module CLI/MCP declarations and zero-effect refusals."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class ModuleEffects(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="module-effects-", dir=SCRATCH)
        self.root = Path(self.scratch.name)
        self.held = self.root / "held-authority.json"
        self.held.write_text('{"success":true,"release":{"caller":"untrusted"}}')
        self.log = self.root / "probe.calls"
        self.engine = self.root / "unconsumed-engine"
        self.engine.write_text("#!/bin/sh\nprintf '%s\\n' probe >> '" + str(self.log) + "'\nexit 93\n")
        self.engine.chmod(0o700)
        self.env = dict(os.environ, DS_SRE_REPORTING="off",
                        DS_NATIVE_LOCAL_GO="1",
                        DS_NATIVE_CLIENT_PROFILE_BUNDLE=str(self.held),
                        DS_SOLAR_ENGINE_BIN=str(self.engine),
                        DS_REPORT_BIN=str(self.engine),
                        DS_NETWORK_ENGINE_BIN=str(self.engine))
        self.args = {"component": "network", "channel": "canary", "sha256": "a" * 64}

    def tearDown(self):
        self.assertFalse(self.log.exists(), "discovery/refusal/selection started an addon")
        self.scratch.cleanup()

    def cli(self, *args):
        result = subprocess.run([str(DS), *args, "--output", "json"], env=self.env,
                                text=True, capture_output=True, timeout=30)
        self.assertTrue(result.stdout, result.stderr)
        return json.loads(result.stdout)

    def mcp(self, command, arguments=None, operation="invoke"):
        inputs = {"operation": operation, "command": command}
        if operation == "invoke":
            inputs["arguments"] = dict(arguments or {})
            if "confirm" in inputs["arguments"]:
                inputs["confirm"] = inputs["arguments"].pop("confirm")
        request = {"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "ds_design", "arguments": inputs}}
        result = subprocess.run([str(DS), "mcp", "serve"], env=self.env,
                                input=json.dumps(request) + "\n", text=True,
                                capture_output=True, timeout=30)
        response = next(json.loads(line) for line in result.stdout.splitlines()
                        if json.loads(line).get("id") == 1)
        self.assertIn("result", response, response)
        return response["result"]["structuredContent"]

    def test_complete_cli_mcp_discovery_has_exact_inputs_and_confirmation(self):
        for command, effect, args, confirmation in [
                ("server.module-stage", "machine_write", ["component", "channel", "sha256"], True),
                ("server.module-selection", "read_only", ["component", "channel"], False)]:
            descriptor = self.cli("capabilities", command)["data"]["command"]
            self.assertEqual(descriptor["effect"], effect)
            self.assertEqual(descriptor["authority"], "none")
            self.assertEqual(descriptor["execution"], "sync")
            self.assertEqual(descriptor["confirmation_required"], confirmation)
            self.assertEqual([arg["name"] for arg in descriptor["inputs"]], args)
            self.assertTrue(all(arg["required"] for arg in descriptor["inputs"]))
            self.assertEqual(self.mcp(command, operation="describe")["data"]["command"], descriptor)
            self.assertEqual(self.cli("server", command.split(".")[1], "--help")
                             ["data"], descriptor)

    def test_missing_confirmation_and_invalid_digest_stop_before_authority(self):
        direct = self.cli("server", "module-stage", "--component", "network",
                          "--channel", "canary", "--sha256", "a" * 64)
        self.assertEqual(direct["error"]["code"], "confirmation_required")
        refused = self.mcp("server.module-stage", self.args)
        self.assertEqual(refused["error"]["code"], "confirmation_required")
        self.assertEqual(self.mcp("server.module-stage", dict(self.args, confirm=False))
                         ["error"]["code"], "confirmation_required")
        for digest in ["bad", "A" * 64, "f" * 63]:
            direct = self.cli("server", "module-stage", "--component", "network", "--channel",
                              "canary", "--sha256", digest, "--yes")
            self.assertEqual(direct["error"]["code"], "module_release_invalid")
            args = dict(self.args, sha256=digest, confirm=True)
            self.assertEqual(self.mcp("server.module-stage", args)["error"]["code"],
                             "module_release_invalid")
        confirmed = self.mcp("server.module-stage", dict(self.args, confirm=True))
        self.assertEqual(confirmed["error"]["code"], "module_authority_unavailable")

    def test_closed_inputs_cannot_admit_catalog_paths_urls_or_guards(self):
        for key, value in [("file", str(self.held)), ("url", "https://example.invalid/a"),
                           ("path", str(self.engine)), ("authority", str(self.held)),
                           ("probe", str(self.engine)), ("idle", True), ("dirty", False)]:
            direct = self.cli("server", "module-stage", "--component", "network", "--channel",
                              "canary", "--sha256", "a" * 64, "--" + key, str(value), "--yes")
            self.assertEqual(direct["error"]["code"], "unknown_flag", key)
            refused = self.mcp("server.module-stage", dict(self.args, confirm=True, **{key: value}))
            self.assertNotEqual(refused.get("status"), "ok", (key, refused))
        for command in ["module-stage", "module-selection"]:
            args = ["server", command, "--component", "network", "--channel", "local"]
            if command == "module-stage":
                args += ["--sha256", "a" * 64, "--yes"]
            self.assertEqual(self.cli(*args)["error"]["code"], "invalid_choice")

    def test_selection_is_local_with_development_profile_and_unused_addons(self):
        for component in ["solar", "reporter", "network"]:
            for channel in ["stable", "canary"]:
                args = {"component": component, "channel": channel}
                direct = self.cli("server", "module-selection", "--component", component,
                                  "--channel", channel)
                self.assertEqual(direct["status"], "ok", direct)
                self.assertEqual(self.mcp("server.module-selection", args)["data"], direct["data"])
                self.assertNotEqual(self.mcp("server.module-selection", dict(args, confirm=True))
                                    .get("status"), "ok")
                if direct["data"] is not None:
                    self.assertEqual(direct["data"]["schema"], "ds.module-selection/v1")
                    self.assertEqual(direct["data"]["component"], component)
                    self.assertEqual(direct["data"]["channel"], channel)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ds", required=True, type=Path)
    parser.add_argument("--scratch", required=True, type=Path,
                        help="existing directory on disk; installed HOME is preserved")
    options = parser.parse_args()
    DS = options.ds.resolve(strict=True)
    SCRATCH = options.scratch.resolve(strict=True)
    unittest.main(argv=[__file__], verbosity=2)

# Codex Remote-SSH UI repair on `ds-server`

Use this runbook when the VS Code Remote-SSH window reports:

> Codex could not start
> The extension could not start its user interface.

The common failure is not the Linux Codex executable. VS Code's remote
extension host runs a newer Node.js runtime where `navigator` is global, while
the extension hits VS Code's migration guard. Codex's backend starts, but the
webview never sends its first ready message.

## Recognize the exact failure

The Codex log contains:

```text
[CodexWebviewProvider] Webview renderer did not become ready
reason=renderer_ready_timeout receivedWebviewMessage=false
```

On a machine that has not yet received the compatibility setting, the matching
`remoteexthost.log` also contains:

```text
PendingMigrationError: navigator is now a global in nodejs
```

On a recurrence after the persistent setting is already installed, there may
be **no new** `PendingMigrationError`. Instead, the current extension-host
command already includes `--supportGlobalNavigator`, the Codex app-server is
running, and only the new activation block ends in `renderer_ready_timeout`.
That is a stale Codex webview/extension-host instance; use the same narrow host
restart below. Do not undo the setting or reinstall Codex.

Find the newest relevant logs without scanning unrelated extension logs:

```bash
find ~/.vscode-server/data/logs -path '*/openai.chatgpt/Codex.log' \
  -printf '%T@ %p\n' | sort -n | tail -1

find ~/.vscode-server/data/logs -path '*/remoteexthost.log' -type f -print0 \
  | xargs -0 grep -El 'PendingMigrationError: navigator is now a global in nodejs' \
  | tail -5
```

Confirm the bundled backend is executable before changing anything:

```bash
codex_bin=$(find ~/.vscode-server/extensions -path \
  '*/openai.chatgpt-*/bin/linux-x86_64/codex' -type f | sort | tail -1)
"$codex_bin" --version
```

If the binary succeeds and either failure shape above matches, do not reinstall
the extension or delete Codex state. Apply (or idempotently confirm) the
compatibility setting, then restart only the owning extension host.

## Persistent repair

Merge the supported VS Code setting into the remote machine settings:

```bash
python3 - <<'PY'
import json
from pathlib import Path

path = Path.home() / ".vscode-server/data/Machine/settings.json"
path.parent.mkdir(parents=True, exist_ok=True)
settings = json.loads(path.read_text()) if path.exists() else {}
settings["extensions.supportNodeGlobalNavigator"] = True
path.write_text(json.dumps(settings, indent=2) + "\n")
PY
```

Then use **Developer: Reload Window** in the affected VS Code Remote-SSH
window. This restarts only that window's extension host and leaves Linux
desktop deployments and other server services alone.

For an unattended repair, an agent may terminate only the extension host that
owns the running Codex app-server. Resolve and validate the parent first; never
kill every VS Code, Node, Cargo, deployment, or server process:

```bash
codex_pid=$(pgrep -n -f \
  '/openai\.chatgpt-[^ ]*/bin/linux-x86_64/codex .*app-server')
host_pid=$(ps -o ppid= -p "$codex_pid" | tr -d ' ')
host_cmd=$(ps -o args= -p "$host_pid")

case "$host_cmd" in
  *'--type=extensionHost'*) kill -TERM "$host_pid" ;;
  *) echo "Refusing to stop unexpected parent PID $host_pid" >&2; exit 1 ;;
esac
```

VS Code should automatically create a replacement extension host. If it does
not, reload the remote window once.

## Verify the repair

The new extension-host command line must include `--supportGlobalNavigator`,
and the Codex app-server must be running:

```bash
ps -u "$USER" -o pid=,args= \
  | grep '[b]ootstrap-fork --type=extensionHost' \
  | grep -- '--supportGlobalNavigator'

pgrep -af \
  '/openai\.chatgpt-[^ ]*/bin/linux-x86_64/codex .*app-server'
```

Wait at least 30 seconds and inspect only the log text written after the latest
`Activating Codex extension` line. Confirm that this new activation block has no
`renderer_ready_timeout`, and that the replacement extension-host log has no
new `PendingMigrationError`. Searching the entire reused Codex log produces a
false failure because the pre-restart timeout remains there as history. A
successful startup commonly logs an authenticated account lookup and a ready
provider.

Plugin-catalog timeouts, telemetry bootstrap warnings, or an isolated HTTP 403
after the renderer is ready are separate network/service issues. They do not
mean this UI repair failed.

## Repair boundaries

- Keep this setting on `ds-server`; it survives ordinary Codex extension updates.
- Reapply it if `~/.vscode-server/data` is rebuilt or removed.
- Do not patch the bundled Codex JavaScript as the first fix.
- Do not delete `~/.codex`, because it contains authentication and user state.
- Do not restart the Linux desktop deployment or the whole host for this failure.

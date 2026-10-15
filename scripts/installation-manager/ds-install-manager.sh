#!/usr/bin/env bash
# Thin Linux bootstrap; component lifecycle remains in installed Core.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
action=${1:-status}; component=${2:-core}; channel=${3:-canary}; consent=${4:-}
[[ "$channel" == stable || "$channel" == canary ]] || { echo 'manager_lane_invalid' >&2;exit 2; }
[[ "$(uname -s):$(uname -m)" == Linux:x86_64 ]] || { echo 'manager_platform_unavailable' >&2;exit 2; }
case "$action" in setup|status|install|update|remove|rollback) ;; *) echo 'manager_action_invalid' >&2;exit 2;;esac
case "$component" in core|desktop|interactions|command-kernel|network|solar|reporter) ;; *) echo 'manager_component_invalid' >&2;exit 2;;esac
python3 - "$root" <<'PY'
import hashlib,json,pathlib,re,sys
root=pathlib.Path(sys.argv[1]);m=json.loads((root/'package.receipt.json').read_bytes())
assert re.fullmatch('[a-f0-9]{40}',m['source_sha']) and re.fullmatch('[a-f0-9]{40}',m['web_source_sha'])
assert m['schema']=='ds.installation-manager-package/v1' and m['platform']=='linux' and m['arch']=='x86_64'
assert {f['filename'] for f in m['files']}=={'ds-install-manager.sh','install-core.sh','install-desktop.sh'} and len(m['files'])==3
for f in m['files']:
 p=root/f['filename'];assert not p.is_symlink() and p.stat().st_size==f['size_bytes'] and hashlib.sha256(p.read_bytes()).hexdigest()==f['sha256']
PY
command=ds; [[ "$channel" != canary ]] || command=ds-canary
if [[ "$action" == setup ]]; then
 [[ "$consent" == --yes ]] || { echo 'confirmation_required' >&2;exit 2; }
 destination="${XDG_DATA_HOME:-$HOME/.local/share}/ds-installation-manager/$channel"
 [[ ! -L "$destination" ]] || { echo 'manager_path_invalid' >&2;exit 2; }
 mkdir -p "$destination";chmod 700 "$destination"
 cp "$root"/{ds-install-manager.sh,install-core.sh,install-desktop.sh,package.receipt.json} "$destination/"
 printf 'Installed DS Installation Manager: bash %q status core %q\n' "$destination/ds-install-manager.sh" "$channel";exit 0
fi
if [[ "$action" == status && ( "$component" == core || "$component" == desktop ) ]];then
 if command -v "$command" >/dev/null;then "$command" version --output json;else printf '{"schema":"ds.installation-manager-status/v1","channel":"%s","core_installed":false}\n' "$channel";fi
 exit 0
fi
[[ "$action" == status || "$consent" == --yes ]] || { echo 'confirmation_required' >&2;exit 2; }
remove_legacy_desktop() {
 local package version status record
 for package in ds-grid-design ds-grid-design-canary;do
  record=$(dpkg-query -W -f='${Status}\t${Version}' "$package" 2>/dev/null) || continue
  IFS=$'\t' read -r status version <<<"$record"
  [[ "$status" == 'install ok installed' && -n "$version" ]] || continue
  dpkg --compare-versions "$version" lt 0.1.7 || continue
  if [[ $EUID -eq 0 ]];then apt-get remove -y --no-auto-remove "$package"
  else sudo -n apt-get remove -y --no-auto-remove "$package";fi
 done
}
ensure_required_ui() {
 local name=$1 selection state captured
 selection=$("$command" core component selection --component "$name" --channel "$channel" --runtime wasm --output json)
 state=$(python3 - "$selection" <<'PY_SELECTION'
import json,sys
x=json.loads(sys.argv[1]);assert x['status']=='ok'
data=x.get('data')
if data is None:print('missing')
else:
 state=data['selection']['state'];assert state in ['installed','removed'],'component_selection_invalid'
 print(state)
PY_SELECTION
 )
 if [[ "$state" == installed ]];then
  # The existing native capture owner verifies all four immutable files and
  # build identity under its updater lock. Never reuse selection JSON alone.
  captured=$("$command" core component capture --component "$name" --channel "$channel" --runtime wasm --output json)
  python3 - "$selection" "$captured" "$name" "$channel" <<'PY_CAPTURE'
import json,re,sys
s,c=map(json.loads,sys.argv[1:3]);assert c['v']==1 and c['command']=='core.component.capture' and c['contract']==1 and c['status']=='ok','component_receipt_invalid'
s=s['data'];c=c['data'];r=c['receipt'];u=r['unit'];release=u['release']
assert r['schema']=='ds.ui-module-installation/v1' and c['generation']==s['generation'],'component_receipt_invalid'
assert re.fullmatch('[a-f0-9]{64}',u['unit_sha256']) and u['unit_sha256']==s['selection']['active']['unit']['unit_sha256'],'component_receipt_invalid'
assert release['component']==sys.argv[3] and release['channel']==sys.argv[4] and release['runtime']=='wasm','component_receipt_invalid'
assert len(release['assets'])==4 and all(re.fullmatch('[a-f0-9]{64}',a['sha256']) and a['size_bytes']>0 for a in release['assets']),'component_receipt_invalid'
PY_CAPTURE
  printf 'Already installed the natively captured %s unit; preserving its generation.\n' "$name"
 else
  bash "$0" install "$name" "$channel" --yes
 fi
}
if [[ "$component" == core || "$component" == desktop ]];then
 [[ "$action" == install || "$action" == update ]] || { echo 'component_lifecycle_unavailable: use the qualified platform owner' >&2;exit 2; }
 if [[ "$component" == desktop ]];then remove_legacy_desktop;DS_GRIDDESIGN_CHANNEL="$channel" bash "$root/install-desktop.sh";fi
 replace_core=0;[[ "$component:$action" != core:update ]] || replace_core=1
 DS_GRIDDESIGN_CORE_REPLACE="$replace_core" DS_GRIDDESIGN_CORE_CATALOG=independent DS_GRIDDESIGN_CHANNEL="$channel" bash "$root/install-core.sh"
 [[ "$component" != desktop ]] || { ensure_required_ui interactions;ensure_required_ui command-kernel; }
 exit 0
fi
command -v "$command" >/dev/null || { echo 'core_missing: install Core first' >&2;exit 2; }
runtime=native;[[ "$component" != interactions && "$component" != command-kernel ]] || runtime=wasm
selection=$("$command" core component selection --component "$component" --channel "$channel" --runtime "$runtime" --output json)
if [[ "$action" == status ]];then printf '%s\n' "$selection";exit 0;fi
generation=$(python3 -c 'import json,sys;x=json.load(sys.stdin);assert x["status"]=="ok";print((x["data"] or {}).get("generation",0))' <<<"$selection")
if [[ "$action" == remove || "$action" == rollback ]];then exec "$command" core component "$action" --component "$component" --channel "$channel" --runtime "$runtime" --expected-generation "$generation" --yes --output json;fi
origin=https://eds-gateway-3c0q477h.ue.gateway.dev;[[ "$channel" != canary ]] || origin=https://eds-gateway-canary-3c0q477h.ue.gateway.dev
work=$(mktemp -d);trap 'rm -rf -- "$work"' EXIT
curl --proto '=https' --max-time 45 -fsS "$origin/api/v1/app_releases?component=$component&channel=$channel&runtime=$runtime" -o "$work/release.json"
admitted=$("$command" core module-release --component "$component" --channel "$channel" --runtime "$runtime" --file "$work/release.json" --output json)
digest=$(python3 - "$runtime" "$work/release.json" "$admitted" <<'PY'
import json,sys
x=json.loads(sys.argv[3]);assert x['status']=='ok'
if sys.argv[1]=='wasm':print(x['data']['unit_sha256'])
else:
 r=json.load(open(sys.argv[2]))['release'];a=[a for a in r['assets'] if a['platform']=='linux' and a['arch']=='x86_64'];assert len(a)==1;print(a[0]['sha256'])
PY
)
# Public admission remains mandatory even when this exact UI unit is active.
# Repeating install must not create a new generation or disturb captured workers.
if [[ "$runtime" == wasm ]] && python3 - "$selection" "$digest" <<'PY_ACTIVE'
import json,sys
r=json.loads(sys.argv[1]);assert r['status']=='ok'
selection=(r.get('data') or {}).get('selection') or {}
active=selection.get('active') or {}
sys.exit(0 if selection.get('state')=='installed' and (active.get('unit') or {}).get('unit_sha256')==sys.argv[2] else 1)
PY_ACTIVE
then
 printf 'Already installed the exact admitted %s unit; preserving generation %s.\n' "$component" "$generation"
 printf '%s\n' "$selection"
 exit 0
fi
ready=$("$command" core module-stage --component "$component" --channel "$channel" --runtime "$runtime" --sha256 "$digest" --yes --output json)
receipt=$(python3 -c 'import json,sys;x=json.load(sys.stdin);assert x["status"]=="ok";print(x["data"]["receiptId"])' <<<"$ready")
"$command" core component activate --component "$component" --channel "$channel" --runtime "$runtime" --receipt-id "$receipt" --expected-generation "$generation" --yes --output json

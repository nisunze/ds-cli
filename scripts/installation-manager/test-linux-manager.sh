#!/usr/bin/env bash
# Lightweight Linux bootstrap qualification; never installs Core or Desktop.
set -euo pipefail
source_root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
python_bin=$(command -v python3)
bash_bin=$(command -v bash)
scratch=$(mktemp -d "${TMPDIR:-/var/tmp}/ds-linux-manager-test.XXXXXXXX")
trap '[[ -n "$scratch" && "$scratch" == */ds-linux-manager-test.* ]] && rm -rf -- "$scratch"' EXIT
mkdir -p "$scratch/bin" "$scratch/home"
# Exact test PATH contains no ds, ds-canary, curl, apt-get or dpkg.
for tool in python3 uname dirname mkdir chmod cp bash; do
  ln -s "$(command -v "$tool")" "$scratch/bin/$tool"
done
export HOME="$scratch/home"
export XDG_DATA_HOME="$scratch/data"
export DS_MANAGER_TEST_OWNER_LOG="$scratch/unexpected-owner.log"
passed=0
failed=0

fixture() {
  package="$scratch/package-$1"
  mkdir "$package"
  cp "$source_root/ds-install-manager.sh" "$package/ds-install-manager.sh"
  for member in install-core.sh install-desktop.sh; do
    cat > "$package/$member" <<'OWNER'
#!/usr/bin/env bash
printf '%s\n' 'unexpected owning installer execution' >> "$DS_MANAGER_TEST_OWNER_LOG"
exit 91
OWNER
  done
  "$python_bin" - "$package" <<'PY'
import hashlib,json,pathlib,sys
root=pathlib.Path(sys.argv[1])
files=[]
for name in ['ds-install-manager.sh','install-core.sh','install-desktop.sh']:
 b=(root/name).read_bytes()
 files.append({'filename':name,'size_bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()})
(root/'package.receipt.json').write_text(json.dumps({'schema':'ds.installation-manager-package/v1','platform':'linux','arch':'x86_64','source_sha':'a'*40,'web_source_sha':'b'*40,'files':files})+'\n')
PY
}

invoke() {
  set +e
  PATH="$scratch/bin" "$bash_bin" "$package/ds-install-manager.sh" "$@" > "$scratch/stdout" 2> "$scratch/stderr"
  result=$?
  set -e
}

pass() { printf 'PASS %s\n' "$1"; passed=$((passed+1)); }
fail() { printf 'FAIL %s (exit %s)\n' "$1" "$result" >&2; failed=$((failed+1)); }
refusal() {
  local label=$1 expected=${2:-}
  if [[ "$result" -ne 0 ]] && { [[ -z "$expected" ]] || grep -Fq -- "$expected" "$scratch/stderr"; }; then pass "$label"; else fail "$label"; fi
}
mutate() {
  "$python_bin" - "$package/package.receipt.json" "$1" <<'PY'
import json,pathlib,sys
p=pathlib.Path(sys.argv[1]);m=json.loads(p.read_text());case=sys.argv[2]
if case=='missing':m['files'].pop()
elif case=='extra':m['files'].append({'filename':'extra.sh','size_bytes':1,'sha256':'0'*64})
elif case=='duplicate':m['files'][2]=m['files'][1].copy()
elif case=='traversal':m['files'][1]['filename']='../install-core.sh'
elif case=='digest':m['files'][1]['sha256']='0'*64
elif case=='size':m['files'][1]['size_bytes']+=1
elif case=='platform':m['platform']='windows'
elif case=='arch':m['arch']='aarch64'
elif case=='schema':m['schema']='ds.installation-manager-package/v0'
elif case=='source':m['source_sha']='sibling-HEAD'
elif case=='web-source':m['web_source_sha']=''
else:raise AssertionError(case)
p.write_text(json.dumps(m)+'\n')
PY
}

for lane in canary stable; do
  fixture "status-$lane"
  invoke status core "$lane"
  if [[ "$result" -eq 0 ]] && "$python_bin" - "$scratch/stdout" "$lane" <<'PY'
import json,sys
m=json.load(open(sys.argv[1]));assert m['schema']=='ds.installation-manager-status/v1' and m['channel']==sys.argv[2] and m['core_installed'] is False
PY
  then pass "status $lane with Core absent"; else fail "status $lane with Core absent"; fi
done

fixture invalid-input
invoke explode core canary; refusal 'invalid action' manager_action_invalid
invoke status unknown canary; refusal 'invalid component' manager_component_invalid
invoke status core production; refusal 'invalid lane' manager_lane_invalid
for action in setup install update remove rollback; do
  invoke "$action" network canary
  refusal "$action requires confirmation" confirmation_required
done
[[ ! -e "$XDG_DATA_HOME" ]] || { result=0; fail 'refused mutations create no installation'; }
invoke status interactions canary; refusal 'missing Core refuses component status' core_missing
invoke install interactions canary --yes; refusal 'missing Core refuses component install' core_missing

for corruption in missing extra duplicate traversal digest size platform arch schema source web-source; do
  fixture "bad-$corruption"
  mutate "$corruption"
  invoke status core canary
  refusal "package $corruption refused"
done
fixture changed-bytes
printf '\n# changed bytes\n' >> "$package/install-core.sh"
invoke status core canary; refusal 'changed package bytes refused'
fixture changed-manager
printf '\n# changed manager bytes\n' >> "$package/ds-install-manager.sh"
invoke status core canary; refusal 'changed manager bytes refused'
fixture linked-member
mv "$package/install-core.sh" "$scratch/linked-owner.sh"
ln -s "$scratch/linked-owner.sh" "$package/install-core.sh"
invoke status core canary; refusal 'linked package member refused'

fixture setup
invoke setup core canary --yes
if [[ "$result" -eq 0 ]] && "$python_bin" - "$package" "$XDG_DATA_HOME/ds-installation-manager/canary" <<'PY'
import pathlib,stat,sys
source,destination=map(pathlib.Path,sys.argv[1:]);assert stat.S_IMODE(destination.stat().st_mode)==0o700
expected={'ds-install-manager.sh','install-core.sh','install-desktop.sh','package.receipt.json'}
assert {p.name for p in destination.iterdir()}==expected
for name in expected: assert (source/name).read_bytes()==(destination/name).read_bytes() and not (destination/name).is_symlink()
PY
then pass 'setup copies exact verified package into disposable XDG home'; else fail 'setup copies exact verified package into disposable XDG home'; fi
package="$XDG_DATA_HOME/ds-installation-manager/canary"
invoke status core canary
if [[ "$result" -eq 0 ]]; then pass 'installed disposable manager runs with Core absent'; else fail 'installed disposable manager runs with Core absent'; fi
if [[ ! -e "$DS_MANAGER_TEST_OWNER_LOG" ]]; then pass 'no owning installer or component execution'; else result=91; fail 'no owning installer or component execution'; fi
# Legacy Desktop tests stop at a fixture installer, before Core/UI acquisition.
# Mock package commands record effects; none can invoke the real package manager.
ln -s "$(command -v dpkg)" "$scratch/bin/dpkg"
cat > "$scratch/bin/dpkg-query" <<'QUERY'
#!/usr/bin/env python3
import os,sys
args=sys.argv[1:];package=os.environ['DS_MANAGER_TEST_PACKAGE']
if package not in args:sys.exit(1)
format=''
for i,arg in enumerate(args):
 if arg.startswith('-f=') or arg.startswith('--showformat='):format=arg.split('=',1)[1]
 elif arg in ['-f','--showformat']:format=args[i+1]
if not format:format='${binary:Package}\t${Version}\n'
for key,value in {'${Version}':os.environ['DS_MANAGER_TEST_VERSION'],'${Status}':'install ok installed','${binary:Package}':package,'${db:Status-Abbrev}':'ii '}.items():format=format.replace(key,value)
sys.stdout.write(format.replace(chr(92)+'t',chr(9)).replace(chr(92)+'n',chr(10)))
QUERY
cat > "$scratch/bin/sudo" <<'SUDO'
#!/usr/bin/env python3
import json,os,sys
with open(os.environ['DS_MANAGER_TEST_PACKAGE_LOG'],'a') as f:f.write(json.dumps(['sudo']+sys.argv[1:])+chr(10))
sys.exit(0)
SUDO
cat > "$scratch/bin/apt-get" <<'APT'
#!/usr/bin/env python3
import json,os,sys
with open(os.environ['DS_MANAGER_TEST_PACKAGE_LOG'],'a') as f:f.write(json.dumps(['apt-get']+sys.argv[1:])+chr(10))
sys.exit(0)
APT
chmod +x "$scratch/bin/dpkg-query" "$scratch/bin/sudo" "$scratch/bin/apt-get"
export DS_MANAGER_TEST_PACKAGE_LOG="$scratch/package-effects.jsonl"
for lane in canary stable; do
  export DS_MANAGER_TEST_PACKAGE=ds-grid-design
  [[ "$lane" != canary ]] || export DS_MANAGER_TEST_PACKAGE=ds-grid-design-canary
  for version in 0.1.6 0.1.7 0.2.0; do
    fixture "legacy-$lane-$version"
    export DS_MANAGER_TEST_VERSION="$version"
    rm -f "$DS_MANAGER_TEST_PACKAGE_LOG"
    invoke install desktop "$lane" --yes
    if [[ "$result" -eq 91 ]] && "$python_bin" - "$DS_MANAGER_TEST_PACKAGE_LOG" "$DS_MANAGER_TEST_PACKAGE" "$version" <<'PY'
import json,pathlib,sys
path=pathlib.Path(sys.argv[1]);package,version=sys.argv[2:]
calls=[json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
if version=='0.1.6':
 assert len(calls)==1,calls
 args=calls[0]
 assert 'remove' in args and 'purge' not in args and package in args,args
 assert not any('core' in a or 'addon' in a or a in ['rm','autoremove'] for a in args),args
 packages=[a for a in args if a.startswith('ds-')]
 assert packages==[package],packages
else:assert calls==[],calls
PY
    then pass "legacy Desktop $lane $version boundary"; else fail "legacy Desktop $lane $version boundary"; fi
  done
done
printf 'Linux manager tests: %s passed, %s failed\n' "$passed" "$failed"
[[ "$failed" -eq 0 ]]

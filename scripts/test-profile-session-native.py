#!/usr/bin/env python3
import argparse, json, pathlib, subprocess
parser=argparse.ArgumentParser(description='Qualify closed native Core Profile source and actual installed Canary Network using a synthetic private package; no Core transport or UI acceptance claim.')
parser.add_argument('--deps',required=True,type=pathlib.Path)
parser.add_argument('--out',required=True,type=pathlib.Path)
parser.add_argument('--package',type=pathlib.Path,help='Optional exact read-only package proof; no payload is logged')
args=parser.parse_args()
cli=pathlib.Path(__file__).resolve().parents[1]
deps=args.deps.resolve(strict=True); fp=deps.parent/'.fingerprint'; out=args.out.resolve(); out.mkdir(parents=True,exist_ok=True)
def graph(lib):
 name,tag=lib.stem.removeprefix('lib').rsplit('-',1); package=name.replace('_','-') if name.startswith('ds_') else name
 return json.loads((fp/f'{package}-{tag}'/f'lib-{name}.json').read_text())
def dep(g,name):
 dig=next(x[3] for x in g['deps'] if x[1]==name); package=name.replace('_','-') if name.startswith('ds_') else name
 for p in fp.glob(f'{package}-*/lib-{name}'):
  if int.from_bytes(bytes.fromhex(p.read_text().strip()),'little')==dig:
   tag=p.parent.name.removeprefix(package+'-'); lib=deps/f'lib{name}-{tag}.rlib'
   if not lib.exists(): lib=deps/f'lib{name}.rlib'
   if lib.exists(): return lib,json.loads(p.with_suffix('.json').read_text())
 raise RuntimeError(name)
# Follow one compiled server closure, including its exact Kernel and display
# runtime. Choosing newest independent libraries can cross an unsuffixed Kernel
# ABI after an independent captured build; no sibling HEAD substitutes qualify.
candidates=sorted([*fp.glob('ds-cli-server-*/lib-ds_cli_server.json'),*fp.glob('ds-cli-server-*/test-lib-ds_cli_server.json')],key=lambda p:p.stat().st_mtime_ns,reverse=True)
selected=next((p for p in candidates if any(x[1]=='ds_profile_runtime' for x in json.loads(p.read_text())['deps'])),None)
if selected is None: raise RuntimeError('Compile the qualified Profile View server source before this native fixture; retained old server closure has no display owner')
sg=json.loads(selected.read_text())
(out/'server-graph-proof.json').write_text(json.dumps({'fingerprint':str(selected),'kind':'test' if selected.name.startswith('test-') else 'library'},indent=2)+'\n')
roots={name:dep(sg,name)[0] for name in ['ds_grid_exchange','ds_network_client','ds_cli_contract','serde','serde_json','ds_grid_engine','sha2','base64','ds_network_contracts','getrandom','ds_profile_runtime','ds_command_kernel']}
src=cli/'crates/ds-cli-server/src/profile_sessions.rs'
command=[str(pathlib.Path.home()/'.cargo/bin/rustc'),'--edition=2024','--test',str(src),'-Ldependency='+str(deps),'-Ccodegen-units=1','-o',str(out/'profile-owner-tests')]
for name,lib in roots.items(): command+=['--extern',f'{name}={lib}']
(out/'dependency-proof.json').write_text(json.dumps({k:str(v) for k,v in roots.items()},indent=2)+'\n')
subprocess.run(command,check=True)
subprocess.run([str(out/'profile-owner-tests'),'--nocapture'],check=True)

fixture=cli/'scripts/tests/profile_session_native.rs'
command=[str(pathlib.Path.home()/'.cargo/bin/rustc'),'--edition=2021',str(fixture),'-Ldependency='+str(deps),'-Ccodegen-units=1','-o',str(out/'native-fixture')]
for name,lib in roots.items(): command+=['--extern',f'{name}={lib}']
subprocess.run(command,check=True)
scratch=out/'scratch';scratch.mkdir(mode=0o700,exist_ok=True)
with (out/'native-process-proof.json').open('w') as log: subprocess.run([str(out/'native-fixture'),str(scratch)]+([str(args.package.resolve(strict=True))] if args.package else []),stdout=log,check=True)
print((out/'native-process-proof.json').read_text())

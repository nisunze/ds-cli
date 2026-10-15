#!/usr/bin/env python3
import argparse, json, pathlib, subprocess
parser=argparse.ArgumentParser(description='Qualify closed native Core Profile source and actual installed Canary Network using a synthetic private package; no Core transport or UI acceptance claim.')
parser.add_argument('--deps',required=True,type=pathlib.Path)
parser.add_argument('--out',required=True,type=pathlib.Path)
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
exchange=max(deps.glob('libds_grid_exchange-*.rlib'),key=lambda p:p.stat().st_mtime_ns); eg=graph(exchange)
jsonlib,jg=dep(eg,'serde_json'); serdelib,_=dep(eg,'serde')
def matching(name):
 for lib in sorted(deps.glob('lib'+name+'-*.rlib'),key=lambda p:p.stat().st_mtime_ns,reverse=True):
  g=graph(lib)
  try:
   j,_=dep(g,'serde_json')
   if j==jsonlib: return lib,g
  except (StopIteration,RuntimeError): continue
 raise RuntimeError('matching '+name)
network,ng=matching('ds_network_client'); contract,cg=matching('ds_cli_contract')
roots={'ds_grid_exchange':exchange,'ds_network_client':network,'ds_cli_contract':contract,'serde':serdelib,'serde_json':jsonlib}
for name in ['ds_grid_engine','sha2','base64']: roots[name]=dep(eg,name)[0]
roots['ds_network_contracts']=dep(ng,'ds_network_contracts')[0]
server,sg=matching('ds_cli_server')
roots['getrandom']=dep(sg,'getrandom')[0]
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
with (out/'native-process-proof.json').open('w') as log: subprocess.run([str(out/'native-fixture'),str(scratch)],stdout=log,check=True)
print((out/'native-process-proof.json').read_text())

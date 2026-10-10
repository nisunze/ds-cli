import pathlib,sys,json,collections,hashlib
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');sys.path.insert(0,str(r/'python-libs'))
code=(r/'gpkg_compare.py').read_text().split('out=[]')[0];exec(code)
from scipy.spatial import cKDTree
scope=json.loads((r/'scope87.json').read_text(encoding='utf-8-sig'))['data']['grouping']['groups'][0]['transformers'];excluded={'zihare','ruhuha','upgrade_mbeho'};plans=[]
for name in scope:
 oldfile=r/'gpkg'/(name+'.gpkg');src=r/'live'/(name+'.json');j=json.loads(src.read_text());features=j['jobs'][0]['gdfs']['lv_poles']['features'];b=live(src)['lv_poles'];changes=[];unmatched=[]
 if name not in excluded and oldfile.exists():
  old=oldread(oldfile)['lv_poles'];tree=cKDTree([(x['g'].x,x['g'].y) for x in old]);used=set();matched=0
  for i,x in enumerate(b):
   candidates=tree.query_ball_point((x['g'].x,x['g'].y),.001)
   if len(candidates)!=1:unmatched.append(features[i]['id']);continue
   oi=candidates[0]
   if oi in used:raise RuntimeError('Duplicate point mapping '+name)
   used.add(oi);matched+=1;before=norm(x['properties'].get('struct_type'));after=norm(old[oi]['properties'].get('struct_type'))
   if before!=after:
    assert after in ('S140','S190','S255');changes.append({'id':features[i]['id'],'old_current':before,'restore':after,'coordinate':features[i]['geometry']['coordinates'],'distance_m':x['g'].distance(old[oi]['g'])})
 else:matched=None
 plans.append({'transformer':name,'source_sha256':hashlib.sha256(src.read_bytes()).hexdigest(),'source_version':json.loads((r/'live'/(name+'.receipt.json')).read_text())['data']['source']['version'],'restore_changes':changes,'pole_count':len(features),'position_matches':matched,'unmatched_positions':unmatched,'restore_excluded':name in excluded or not oldfile.exists()})
(r/'pole-restore-plan.json').write_text(json.dumps({'scope':scope,'position_tolerance_m':.001,'purpose':'Floating-point normalization only; no nearest-neighbour relocation matching','transformers':plans},indent=2))
print(json.dumps({'scope':len(scope),'restore_sites':sum(bool(x['restore_changes']) for x in plans),'restore_poles':sum(len(x['restore_changes']) for x in plans),'sites':[{'name':x['transformer'],'changes':len(x['restore_changes']),'matched_positions':x['position_matches'],'unmatched_live_positions':len(x['unmatched_positions'])} for x in plans if x['restore_changes'] or (x['unmatched_positions'] and not x['restore_excluded'])]},indent=2))

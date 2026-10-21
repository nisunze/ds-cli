import pathlib,json,sys,zipfile,collections,hashlib
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');sys.path.insert(0,str(r/'python-libs'))
with zipfile.ZipFile(r/'approved-restore-verification.zip') as z:z.extractall(r/'restoration')
code=(r/'gpkg_compare.py').read_text().split('out=[]')[0];exec(code)
plan=json.loads((r/'restoration/pole-restore-plan.json').read_text());work=r/'restoration/approved-restore';features=0;restored=0;matched=0;unmatched=0;mismatches=[];proofs=[]
for p in plan['transformers']:
 name=p['transformer'];folder=work/name;proof=json.loads((folder/'verified.json').read_text());proofs.append(proof);restored+=proof['restored_poles'];j=json.loads((folder/'after.json').read_text())['jobs'][0]['gdfs']
 for layer,v in j.items():
  for f in v['features']:
   assert f.get('properties',{}).get('drafting_status')=='approved',(name,layer,f.get('id'));features+=1
 oldfile=r/'gpkg'/(name+'.gpkg')
 if p['restore_excluded']:continue
 old=oldread(oldfile)['lv_poles'];new=live(folder/'after.json')['lv_poles'];tree=cKDTree([(x['g'].x,x['g'].y) for x in old])
 for f in new:
  ii=tree.query_ball_point((f['g'].x,f['g'].y),.001)
  if len(ii)!=1:unmatched+=1;continue
  matched+=1
  if norm(old[ii[0]]['properties'].get('struct_type'))!=norm(f['properties'].get('struct_type')):mismatches.append(name)
assert len(proofs)==87 and restored==301 and not mismatches
assert all(x['counts_unchanged'] and x['geometry_unchanged'] for x in proofs)
summary={'project':'czgmdwth_gisagara','scope':'initial87','transformers_saved_and_verified':87,'approved_features':features,'restored_pole_types':restored,'restored_transformers':sum(bool(x['restored_poles']) for x in proofs),'matching_positions_checked':matched,'remaining_type_differences_at_matching_positions':0,'unmatched_live_positions_left_unchanged':unmatched,'all_geometry_unchanged':True,'all_counts_unchanged':True,'position_match_tolerance_m':.001,'position_tolerance_purpose':'floating-point normalization only','restore_excluded':['zihare','ruhuha','upgrade_mbeho'],'no_baseline':['substation'],'external_dependencies':['Python','SQLite','Shapely','SciPy','pyproj'],'integration_feedback':'959dd4d6-c75c-44da-b54a-7689cf59d7e4','readiness_feedback':'2a0f9de2-a0c9-43e0-8441-31dea6460c32','mutations':'DS CLI map.design.set and map.design.save only'}
(r/'pole-type-restoration-receipt.json').write_text(json.dumps(summary,indent=2));print(json.dumps(summary,indent=2))

import json,pathlib,collections
r=pathlib.Path('/home/magese/projects/gisagara/refreshed87-20261010');res=json.loads((r/'integrity-process-result.json').read_text());out=[]
for x in res['jobs']:
 n=x['transformer_name'];o=x.get('output',{});warnings=o.get('warnings',[]);old=json.loads((r/'inputs'/(n+'.json')).read_text())['jobs'][0]['gdfs'];g=o.get('gdfs',{});protected=[]
 for layer in old:
  if layer not in g:protected.append(layer+':missing');continue
  bm={str(f.get('id',i)):f for i,f in enumerate(old[layer]['features'])};am={str(f.get('id',i)):f for i,f in enumerate(g[layer]['features'])}
  if bm.keys()!=am.keys():protected.append(layer+':feature-identity-changed');continue
  for fid,f in bm.items():
   a=am[fid]
   if f['geometry']!=a['geometry']:protected.append(layer+':geometry:'+fid)
   for key in ['struct_type','assembly_type','stay','earthing','plan_kva','tr_size','drafting_status','pole_number','house_number','cable_size']:
    if f['properties'].get(key)!=a['properties'].get(key):protected.append(layer+':'+key+':'+fid)
 out.append({'transformer':n,'ok':x['ok'],'warnings':warnings,'protected_change_candidates':protected,'native_preservation':x.get('preservation',{})})
(r/'native-integrity-review.json').write_text(json.dumps(out,indent=2));print('NATIVE',json.dumps([{'transformer':x['transformer'],'ok':x['ok'],'warnings':x['warnings'],'protected_changes':len(x['protected_change_candidates'])} for x in out]))

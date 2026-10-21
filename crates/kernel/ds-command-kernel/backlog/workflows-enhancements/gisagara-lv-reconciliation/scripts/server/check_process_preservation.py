import pathlib,json,collections
r=pathlib.Path('/home/magese/projects/gisagara/refreshed87-20261010');q=json.loads((r/'integrity-process-result.json').read_text());x=next(x for x in q['jobs'] if x['transformer_name']=='fill_in_ep_gisanze');b=json.loads((r/'inputs/fill_in_ep_gisanze.json').read_text())['jobs'][0]['gdfs'];print('TR_BEFORE',[(f['properties'].get('tr_size'),f['properties'].get('plan_kva')) for f in b['tr']['features']]);print('TR_AFTER',[(f['properties'].get('tr_size'),f['properties'].get('plan_kva')) for f in x['output']['gdfs']['tr']['features']]);print('NATIVE_PROTECTED',x['preservation'].get('protected_changes'))
for x in q['jobs']:
 n=x['transformer_name'];before=json.loads((r/'inputs'/(n+'.json')).read_text())['jobs'][0]['gdfs'];after=x['output']['gdfs'];bad=[]
 for layer in before:
  a=collections.Counter(json.dumps(f['geometry'],sort_keys=True) for f in before[layer]['features']);b=collections.Counter(json.dumps(f['geometry'],sort_keys=True) for f in after.get(layer,{}).get('features',[]))
  if a!=b:bad.append(layer)
 if bad:print('GEOMETRY_SET_CHANGED',n,bad)

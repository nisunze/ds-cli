import pathlib,json,collections
b=pathlib.Path('/home/magese/projects/gisagara');ns={};exec((b/'gpkg-comparison-20261010/gpkg_compare.py').read_text().split('out=[]')[0],ns);r=b/'refreshed87-20261010';old=ns['oldread'](b/'gpkg-comparison-20261010/gpkg/upgrade_rebero.gpkg');now=ns['live'](r/'inputs/upgrade_rebero.json');src=json.loads((r/'inputs/upgrade_rebero.json').read_text())['jobs'][0]['gdfs'];out=[]
for i,f in enumerate(now['lv_lines']):
 if f['properties'].get('cable_size'):continue
 matches=[x for x in old['lv_lines'] if f['g'].hausdorff_distance(x['g'])<=.001 and abs(f['g'].length-x['g'].length)<=.001];spans=[x for x in now['spans'] if x['g'].difference(f['g'].buffer(.01)).length<=.01];sizes=collections.Counter(x['properties'].get('cable_size',x['properties'].get('cab_size')) for x in spans);out.append({'id':src['lv_lines']['features'][i]['id'],'length_m':round(f['g'].length,2),'april_exact_matches':[x['properties'].get('cable_size') for x in matches],'contained_span_sizes':dict(sizes)})
(r/'rebero-missing-size-evidence.json').write_text(json.dumps(out,indent=2));print(json.dumps(out))

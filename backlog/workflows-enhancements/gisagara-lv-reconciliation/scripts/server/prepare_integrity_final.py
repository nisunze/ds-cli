import pathlib,json,copy
r=pathlib.Path('/home/magese/projects/gisagara/refreshed87-20261010');q=json.loads((r/'request.json').read_text())
for item in q['transformers']:
 g=json.loads((r/'inputs'/(item['transformer']+'.json')).read_text())['jobs'][0]['gdfs']
 for f in g.get('tr',{}).get('features',[]):
  p=f['properties'];v=p.get('plan_kva')
  if isinstance(v,(int,float)) and v>0:p['tr_size']=v
 f=r/(item['transformer']+'-final-display.json');f.write_text(json.dumps({'layers':g}));item['layers']=str(f)
q['out_dir']=str(r/'final-native');(r/'final-request.json').write_text(json.dumps(q));s=copy.deepcopy(q);s['out_dir']=str(r/'final-supply-native')
for item in s['transformers']:
 if item['transformer']=='kirwa':
  g=json.loads(pathlib.Path(item['layers']).read_text())
  for f in g['layers']['tr']['features']:f['properties']['tr_size']=None
  p=r/'kirwa-final-supply.json';p.write_text(json.dumps(g));item['layers']=str(p)
(r/'final-supply-request.json').write_text(json.dumps(s));f=(r/'finalize.py').read_text().replace("r/'supply-native/combined_transformer.xlsx'","r/'final-supply-native/combined_transformer.xlsx'").replace("r/'native/combined_transformer.shp.zip'","r/'final-native/combined_transformer.shp.zip'");(r/'finalize-final.py').write_text(f)

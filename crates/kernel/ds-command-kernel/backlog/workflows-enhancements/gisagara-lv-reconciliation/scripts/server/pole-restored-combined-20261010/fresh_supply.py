import pathlib,json,copy
r=pathlib.Path('/home/magese/projects/gisagara/pole-restored-combined-20261010');q=json.loads((r/'request.json').read_text())
targets={'upgrade_mbeho':100,'upgrade_kimana':160,'updrade_bazenga':160,'rugarama':160}
for item in q['transformers']:
 n=item['transformer']
 d=json.loads(pathlib.Path(item['layers']).read_text());p=d['layers']['tr']['features'][0]['properties']
 if n in targets:assert p['plan_kva']==targets[n]
 if float(p.get('plan_kva') or 0)>0:p['tr_size']=p['plan_kva']
 f=r/(n+'-supply.json');f.write_text(json.dumps(d));item['layers']=str(f)
q['out_dir']=str(r/'planned-native');(r/'planned-request.json').write_text(json.dumps(q,indent=2))
kirwa=next(x for x in q['transformers'] if x['transformer']=='kirwa');d=json.loads(pathlib.Path(kirwa['layers']).read_text());d['layers']['tr']['features'][0]['properties']['tr_size']=None;f=r/'kirwa-zero-supply.json';f.write_text(json.dumps(d));kirwa['layers']=str(f)
q['out_dir']=str(r/'planned-supply-native');(r/'planned-supply-request.json').write_text(json.dumps(q,indent=2))

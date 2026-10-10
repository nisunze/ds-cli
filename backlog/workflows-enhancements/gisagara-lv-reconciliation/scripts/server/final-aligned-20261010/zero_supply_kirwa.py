import json,pathlib
r=pathlib.Path('/home/magese/projects/gisagara/final-aligned-20261010');p=json.loads((r/'kirwa-report-input.json').read_text())
p['layers']['tr']['features'][0]['properties']['tr_size']=0
(r/'kirwa-zero-supply-input.json').write_text(json.dumps(p))
q=json.loads((r/'request.json').read_text())
for i in q['transformers']:
 if i['transformer']=='kirwa':i['layers']=str(r/'kirwa-zero-supply-input.json')
q['out_dir']=str(r/'zero-supply-native');(r/'zero-supply-request.json').write_text(json.dumps(q,indent=2))

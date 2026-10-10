import json,pathlib
r=pathlib.Path('/home/magese/projects/gisagara/final-aligned-20261010');p=json.loads((r/'kirwa-report-input.json').read_text())
p['layers']['tr']['features'][0]['properties']['tr_size']=None
(r/'kirwa-retained-supply-input.json').write_text(json.dumps(p))
q=json.loads((r/'request.json').read_text())
for i in q['transformers']:
 if i['transformer']=='kirwa':i['layers']=str(r/'kirwa-retained-supply-input.json')
q['out_dir']=str(r/'retained-supply-native');(r/'retained-supply-request.json').write_text(json.dumps(q,indent=2))


import json,pathlib
r=pathlib.Path('/home/magese/projects/gisagara/final-aligned-20261010');p=json.loads((r/'kirwa-report-input.json').read_text())
tr=p['layers']['tr']['features'][0]['properties']
tr['name']='FILL_IN_KIRWA';tr['transfo']='FILL_IN_KIRWA'
(r/'kirwa-classified-display-input.json').write_text(json.dumps(p))
q=json.loads((r/'request.json').read_text())
for i in q['transformers']:
 if i['transformer']=='kirwa':i['layers']=str(r/'kirwa-classified-display-input.json')
q['out_dir']=str(r/'classified-native');(r/'classified-request.json').write_text(json.dumps(q,indent=2))
(r/'delivery-bridge.json').write_text(json.dumps({'feedback':'feae1d8e-271a-4a27-96cc-459b53c50ba5','projection':'Kirwa report display name/transfo = FILL_IN_KIRWA to convey saved fill_in nature to prefix-dependent quantity calculation. Immutable identity kirwa and 100 kVA ratings unchanged. No final workbook edits.','existing_approved_plan_bridge':'6b369e94-89f2-43a0-a8c5-74b8fab0dcf7'},indent=2))

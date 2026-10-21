import json,pathlib,copy
r=pathlib.Path('/home/magese/projects/gisagara');o=r/'final-aligned-20261010';o.mkdir(exist_ok=True)
b=json.loads((o/'kirwa-before.json').read_text(encoding='utf-8-sig'));a=json.loads((o/'kirwa-after.json').read_text(encoding='utf-8-sig'))
bg=b['jobs'][0]['gdfs'];ag=a['jobs'][0]['gdfs'];bp=bg['tr']['features'][0]['properties'];ap=ag['tr']['features'][0]['properties']
changes={k:[bp.get(k),ap.get(k)] for k in bp.keys()|ap.keys() if bp.get(k)!=ap.get(k)}
assert changes=={'transformer_nature':['new_transformer','fill_in'],'transfo_type':['new','fill_in']},changes
bc=copy.deepcopy(bg);ac=copy.deepcopy(ag)
for key in changes:bc['tr']['features'][0]['properties'].pop(key);ac['tr']['features'][0]['properties'].pop(key)
assert bc==ac,'Unrelated layer changes'
req=json.loads((r/'june-approved-20261010/effective-request.json').read_text())
for item in req['transformers']:
 if item['transformer']=='kirwa':
  doc=json.loads(pathlib.Path(item['layers']).read_text());doc['layers']['tr']=ag['tr'];p=o/'kirwa-report-input.json';p.write_text(json.dumps(doc));item['layers']=str(p)
req['transformer_natures']['kirwa']='fill_in';req['out_dir']=str(o/'native')
(o/'request.json').write_text(json.dumps(req,indent=2))
(o/'classification-proof.json').write_text(json.dumps({'changes':changes,'unrelated_layers_unchanged':True,'owner_decision':'Retain existing; no transformer supply','scope_sites':len(req['transformers'])},indent=2))
print({k:v for k,v in req.items() if k not in ['transformers','transformer_natures']})

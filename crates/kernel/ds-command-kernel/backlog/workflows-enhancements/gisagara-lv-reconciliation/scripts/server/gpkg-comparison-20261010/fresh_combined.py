import pathlib,json
r=pathlib.Path('/home/magese/projects/gisagara');o=r/'pole-restored-combined-20261010';o.mkdir(exist_ok=True)
q=json.loads((r/'final-aligned-20261010/request.json').read_text());p=json.loads((r/'gpkg-comparison-20261010/restoration/pole-restore-plan.json').read_text())
assert set(p['scope'])=={x['transformer'] for x in q['transformers']}
for item in q['transformers']:
 name=item['transformer'];j=json.loads((r/'gpkg-comparison-20261010/restoration/approved-restore'/name/'after.json').read_text())['jobs'][0]
 f=o/(name+'.json');f.write_text(json.dumps({'layers':j['gdfs']}));item['layers']=str(f)
q['out_dir']=str(o/'native');(o/'request.json').write_text(json.dumps(q,indent=2))

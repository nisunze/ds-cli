import pathlib,json,copy,hashlib
root=pathlib.Path('/home/magese/projects/gisagara');out=root/'june-approved-20261010';out.mkdir(exist_ok=True)
targets={'upgrade_mbeho':100,'upgrade_kimana':160,'updrade_bazenga':160,'rugarama':160}
req=json.loads((root/'combined87-20261010/request.json').read_text());proof=[]
for name,kva in targets.items():
 before=json.loads((out/(name+'-before.json')).read_text())['jobs'][0]['gdfs']
 after=json.loads((out/(name+'-after.json')).read_text())['jobs'][0]['gdfs']
 expected=copy.deepcopy(before);expected['tr']['features'][0]['properties']['plan_kva']=kva
 assert expected==after,(name,'change beyond plan_kva')
 row=next(x for x in req['transformers'] if x['transformer']==name)
 doc=json.loads(pathlib.Path(row['layers']).read_text())
 doc['layers']['tr']['features'][0]['properties']['plan_kva']=kva
 path=out/(name+'-report-input.json');path.write_text(json.dumps(doc))
 row['layers']=str(path)
 proof.append({'transformer':name,'plan_kva':kva,'only_plan_kva_changed_in_project':True,'report_input_sha256':hashlib.sha256(path.read_bytes()).hexdigest()})
req['transformer_natures']['upgrade_kimana']='upgrade'
req['out_dir']=str(out/'native');(out/'request.json').write_text(json.dumps(req,indent=2));(out/'plan-kva-proof.json').write_text(json.dumps(proof,indent=2))
print(json.dumps(proof))

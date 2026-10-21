import pathlib,json,zipfile,hashlib
r=pathlib.Path('tmp/gpkg-comparison-20261010');p=json.loads((r/'pole-restore-plan.json').read_text());work=r/'approved-restore';proofs=[json.loads((work/n/'verified.json').read_text()) for n in p['scope']]
assert len(proofs)==87 and sum(x['restored_poles'] for x in proofs)==301
assert all(x['counts_unchanged'] and x['geometry_unchanged'] for x in proofs)
with zipfile.ZipFile(r/'approved-restore-verification.zip','w',zipfile.ZIP_DEFLATED) as z:
 z.write(r/'pole-restore-plan.json','pole-restore-plan.json')
 for name in p['scope']:
  for file in (work/name).glob('*.json'):z.write(file,'approved-restore/'+name+'/'+file.name)
print('Packed87 verified designs',hashlib.sha256((r/'approved-restore-verification.zip').read_bytes()).hexdigest())

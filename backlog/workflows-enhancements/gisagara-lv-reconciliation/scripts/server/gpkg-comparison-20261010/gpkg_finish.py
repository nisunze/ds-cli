import pathlib,json,hashlib,shutil
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');a=json.loads((r/'final-comparison.json').read_text())
for x in a:
 if x['baseline']!='Matched':continue
 if x['pole_types']['old']!=x['pole_types']['live'] or x['service_cables']['old_count']!=x['service_cables']['live_count']:
  print(x['transformer'],json.dumps({'types':x['pole_types'],'LV lines':x['counts']['lv_lines'],'service cables':x['counts']['service_cables']}))
p=r/'Gisagara - GeoPackage comparison - initial scope.txt';shutil.copyfile(p,r/'comparison.txt')
(r/'delivery-proof.json').write_text(json.dumps({'file':p.name,'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'scope':84,'matched':83,'excluded':['zihare','ruhuha','upgrade_mbeho']}))
assert len(a)==84 and not {'zihare','ruhuha','upgrade_mbeho'}&{x['transformer'] for x in a}

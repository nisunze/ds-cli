import pathlib,json,collections
b=pathlib.Path('/home/magese/projects/gisagara');ns={};exec((b/'gpkg-comparison-20261010/gpkg_compare.py').read_text().split('out=[]')[0],ns);old=ns['oldread'](b/'gpkg-comparison-20261010/gpkg/rwegera.gpkg');now=ns['live'](b/'rwegera-after-april-restore.json');checks={}
def key(g):
 if g.geom_type=='Point':return tuple(round(v,3) for v in g.coords[0])
 if g.geom_type=='LineString':
  v=tuple(tuple(round(x,3) for x in p) for p in g.coords);return min(v,v[::-1])
 return g.normalize().wkb_hex
for layer in ['tr','lv_poles','lv_lines','customers','service_cables','spans']:
 a=collections.Counter(key(x['g']) for x in old[layer]);z=collections.Counter(key(x['g']) for x in now[layer]);assert a==z,(layer,'geometry mismatch');checks[layer]={'april':len(old[layer]),'saved':len(now[layer]),'geometry_parity':True}
# Verify engineering fields at exact geometry without relying on renumbered IDs.
fields={'lv_poles':['struct_type','assembly_type','stay','earthing','pole_number'],'lv_lines':['cable_size'],'service_cables':['cable_size','pole_number'],'customers':['category','meter_type','pole_number','house_number'],'spans':['cable_size']};mapped={}
for layer,fs in fields.items():
 for field in fs:
  a=collections.Counter((key(x['g']),ns['norm'](x['properties'].get(field))) for x in old[layer]);z=collections.Counter((key(x['g']),ns['norm'](x['properties'].get(field))) for x in now[layer]);assert a==z,(layer,field,'attribute mismatch');mapped[layer+'.'+field]=True
j=json.loads((b/'rwegera-after-april-restore.json').read_text());g=j['jobs'][0]['gdfs'];tr=g['tr']['features'][0]['properties'];assert tr['plan_kva']==50 and tr['tr_size']==50;assert all(f['properties']['drafting_status']=='approved' for fc in g.values() for f in fc['features'])
proof={'transformer':'rwegera','saved':True,'removed_extra_poles':48,'april_geometry_parity':checks,'april_engineering_field_parity':mapped,'plan_kva':50,'tr_size':50,'all_features_approved':True,'processing_invoked':False};(b/'rwegera-april-restoration-proof.json').write_text(json.dumps(proof,indent=2));print(json.dumps(proof))

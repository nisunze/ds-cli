import pathlib,json,collections
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');j=json.loads((r/'live/kibungo.json').read_text())['jobs'][0]['gdfs']['lv_poles']['features'];a=[x for x in j if not x['properties'].get('struct_type')]
for x in a[:5]:print(json.dumps({k:v for k,v in x['properties'].items() if k in ['pole_number','pole_type','struct_type','assembly_type','existing_status','position','drafting_status','stay','earthing','node_kind','source_feature_id']}))
print('count',len(a))

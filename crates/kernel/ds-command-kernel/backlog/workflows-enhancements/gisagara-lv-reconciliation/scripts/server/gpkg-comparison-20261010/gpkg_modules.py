import importlib,json,pathlib
for n in ['shapely','pyproj','scipy','pandas','reportlab','openpyxl']:
 try:m=importlib.import_module(n);print(n,getattr(m,'__version__','available'))
 except ImportError:print(n,'missing')
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');g=json.loads((r/'group-projection.json').read_text(encoding='utf-8-sig'))
print('GROUP KEYS',list(g['data']))
doc=json.loads(g['data']['document']);print('DOC KEYS',list(doc));print('DOC SAMPLE',str(doc)[:1600])

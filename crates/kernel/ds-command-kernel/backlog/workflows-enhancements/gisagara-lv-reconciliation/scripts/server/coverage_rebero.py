import pathlib,json
b=pathlib.Path('/home/magese/projects/gisagara');ns={};exec((b/'gpkg-comparison-20261010/gpkg_compare.py').read_text().split('out=[]')[0],ns);r=b/'refreshed87-20261010';g=ns['live'](r/'inputs/upgrade_rebero.json');from shapely.ops import unary_union
for f in g['lv_lines']:
 if not f['properties'].get('cable_size'):
  spans=[x['g'] for x in g['spans'] if x['g'].difference(f['g'].buffer(.01)).length<=.01];u=unary_union(spans);print(f['properties'].get('line_number'),'uncovered_m',f['g'].difference(u.buffer(.01)).length,'span_sum',sum(x.length for x in spans))

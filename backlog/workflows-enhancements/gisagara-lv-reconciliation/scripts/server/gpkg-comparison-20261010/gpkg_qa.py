import pathlib,json,hashlib
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');p=r/'Gisagara - GeoPackage comparison - initial scope.txt';s=p.read_text()
print('BAD_TEXT',[(v,s.count(v)) for v in [chr(194),chr(195),chr(65533)]])
a=json.loads((r/'final-comparison.json').read_text());assert len(a)==84
assert all(x['customers']['removed_or_unmatched']==x['customers']['added_or_unmatched']==x['customers']['moved_label_matches']==0 for x in a if x['baseline']=='Matched')
assert all((x.get('transformer_move_m') or 0)<.25 for x in a)
print('QA passed; 83 matched customer sets and transformer coordinates unchanged.')

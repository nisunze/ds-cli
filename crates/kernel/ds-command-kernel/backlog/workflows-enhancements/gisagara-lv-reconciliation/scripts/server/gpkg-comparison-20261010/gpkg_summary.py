import pathlib,json,collections
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');a=json.loads((r/'comparison.json').read_text());m=[x for x in a if x['baseline']=='Matched']
print('TOTALS',json.dumps({k:[sum(x['counts'][k][i] for x in m) for i in (0,1)] for k in ('lv_poles','customers','lv_lines','service_cables')}))
print('TR_RAW_DIFFERENCES',[(x['transformer'],x['old_kva'],x['live_designed_kva'],x['live_kva']) for x in m if x['old_kva']!=x['live_designed_kva']])
print('ATTR_SAMPLES',json.dumps(m[0]['poles']['examples'][:15]))
for n in ['fill_in_kagoma','upgrade_mbeho','upgrade_school_transformer','kirwa']:
 d=json.loads((r/'live'/(n+'.json')).read_text())['jobs'][0]['gdfs']['tr']['features'][0]['properties']
 print('TR',n,json.dumps({k:v for k,v in d.items() if k in ['name','tr_size','transfo_size','plan_kva','ex_tr_size','transformer_nature','transformer_supply_quantity']}))
print('CUSTOMER_DIFFERENCES',[(x['transformer'],x['customers']['removed_or_unmatched'],x['customers']['added_or_unmatched'],x['customers']['moved_label_matches']) for x in m if x['customers']['removed_or_unmatched'] or x['customers']['added_or_unmatched'] or x['customers']['moved_label_matches']])

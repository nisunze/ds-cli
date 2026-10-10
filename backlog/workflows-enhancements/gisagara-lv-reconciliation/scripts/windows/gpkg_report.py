import pathlib,json,collections,hashlib,math
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');exclude={'zihare','ruhuha','upgrade_mbeho'}
a=[x for x in json.loads((r/'comparison.json').read_text()) if x['transformer'] not in exclude];assert len(a)==84
m=[x for x in a if x['baseline']=='Matched'];assert len(m)==83
receipts=json.loads((r/'gpkg-byte-receipts.json').read_text());assert len(receipts)==85
for j in receipts:
 d=j['data'];p=r/'gpkg'/d['file_name'];assert hashlib.sha256(p.read_bytes()).hexdigest()==d['sha256']
for x in a:
 name=x['transformer'];d=json.loads((r/'live'/(name+'.receipt.json')).read_text())['data'];assert hashlib.sha256((r/'live'/(name+'.json')).read_bytes()).hexdigest()==d['request_sha256']
def label(s):return s.replace('_',' ').title()
def fmt(n):return f'{n:,.0f}' if n==round(n) else f'{n:,.2f}'
def oldnew(p):return f'{p[0]} -> {p[1]}'
totals={k:[sum(x['counts'][k][i] for x in m) for i in (0,1)] for k in ('lv_poles','customers','lv_lines','service_cables')}
routes=[x for x in m if x['lv_lines']['removed_route_m']>1 or x['lv_lines']['added_route_m']>1]
counts=[x for x in m if x['poles']['old']!=x['poles']['live']]
structure=[x for x in m if x['pole_types']['old']!=x['pole_types']['live']]
linecable=[x for x in m if x['line_cable_changes']]
numbered=[x for x in m if x['poles']['changed_fields'].get('pole_number')]
trmoved=[(x['transformer'],x['transformer_move_m']) for x in m if (x['transformer_move_m'] or 0)>.25]
summary={'scope':84,'matched':83,'no_baseline':['substation'],'excluded':sorted(exclude),'totals':totals,'pole_count_changes':[(x['transformer'],x['poles']['old'],x['poles']['live']) for x in counts],'route_changes':[(x['transformer'],x['lv_lines']['removed_route_m'],x['lv_lines']['added_route_m']) for x in routes],'pole_type_sites':len(structure),'cable_change_sites':[(x['transformer'],len(x['line_cable_changes'])) for x in linecable],'renumbered_sites':len(numbered),'transformer_moves_m':trmoved}
(r/'final-summary.json').write_text(json.dumps(summary,indent=2))
(r/'final-comparison.json').write_text(json.dumps(a,indent=2))
text=['GISAGARA: APRIL GEOPACKAGE TO LIVE PROJECT COMPARISON','10 October 2026','',
'Scope: initial 87 only; exclude Zihare, Ruhuha and Upgrade Mbeho as instructed.',
'84 sites reviewed: 83 matched April GeoPackages; Substation has no April baseline.',
'No additional-scope sites are included. No source designs were modified.','',
'SUMMARY',f"Poles: {oldnew(totals['lv_poles'])} (net +{totals['lv_poles'][1]-totals['lv_poles'][0]}).",f"Customers: {oldnew(totals['customers'])}; all matched customer locations unchanged.",f"LV line features: {oldnew(totals['lv_lines'])}. Feature counts can change through splitting/merging.",f"Service cable features: {oldnew(totals['service_cables'])}.",f'Pole labels changed at {len(numbered)} sites; pole type distributions changed at {len(structure)} sites.',
'', 'ITEM FOR REVIEW',
'Kibungo: 211 poles in both files; 17 live poles have blank/missing struct_type where the April type distribution had 17 more S140 poles. Live S140: 81 -> 64; S190: 11 -> 11; S255: 119 -> 119; untyped: 0 -> 17. These 17 features are marked new. Platform validation feedback filed; no repair inferred or applied.',
'', 'POLE COUNT CHANGES (April -> live)']
for x in counts:text.append(f"  {label(x['transformer'])}: {x['poles']['old']} -> {x['poles']['live']} ({x['poles']['live']-x['poles']['old']:+d})")
text+=['','LV ROUTE CHANGES (metres outside the other route at 0.5 m tolerance)']
for x in routes:text.append(f"  {label(x['transformer'])}: April-only {fmt(x['lv_lines']['removed_route_m'])} m; live-only {fmt(x['lv_lines']['added_route_m'])} m")
text+=['','HOW TO READ THE DETAIL',
'Each site lists April -> live counts and quantities. Pole coordinate matches use 0.25 m tolerance.',
'Unmatched old/live pole positions may represent relocation, replacement or changed sampling; they are not proof of physical removal/addition.',
'Pole types are compared by quantity independently of pole labels. Renumbering does not itself change quantities.',
'Line geometry is compared as merged routes, so splitting and reversing lines do not create route differences.',
'Cable length totals use geometric lengths, not BOQ supply quantities. Differences in calculated fields are reported separately.',
'A zero live tr_size/plan_kva at a retained existing site does not establish a zero physical transformer rating.',
'Positive plan_kva overrides the designed tr_size. No transformer supply is inferred from these fields alone.',
'The April files do not contain the current scope/nature/upgrade-works tags; tag changes cannot be established from this baseline.',
'','PER TRANSFORMER']
for x in a:
 n=x['transformer'];text+=['',label(n)+' ['+n+']']
 if x['baseline']!='Matched':text.append('  No April GeoPackage: no before/after comparison possible.');continue
 p=x['poles'];c=x['customers'];l=x['lv_lines'];s=x['service_cables']
 text.append(f"  Poles {p['old']} -> {p['live']}; customers {c['old']} -> {c['live']}; LV lines {l['old_count']} -> {l['live_count']}; service cables {s['old_count']} -> {s['live_count']}.")
 types=sorted(set(x['pole_types']['old'])|set(x['pole_types']['live']))
 text.append('  Pole types: '+ '; '.join(f"{t or '(blank)'} {x['pole_types']['old'].get(t,0)} -> {x['pole_types']['live'].get(t,0)}" for t in types)+'.')
 text.append(f"  Pole labels changed: {p['changed_fields'].get('pole_number',0)} matched positions. Unmatched old/live positions: {p['removed_or_unmatched']}/{p['added_or_unmatched']}; moved label matches: {p['moved_label_matches']}.")
 attrs={k:v for k,v in p['changed_fields'].items() if k not in ('pole_number','num_houses')}
 text.append('  Pole attributes changed: '+(', '.join(f'{k} ({v})' for k,v in attrs.items()) or 'none on matched poles')+'.')
 text.append(f"  LV geometric length: {fmt(l['old_length_m'])} -> {fmt(l['live_length_m'])} m; April-only route {fmt(l['removed_route_m'])} m; live-only route {fmt(l['added_route_m'])} m.")
 if x['line_cable_changes']:
  cc=collections.Counter((z['old_cable'],z['live_cable']) for z in x['line_cable_changes'])
  text.append('  Cable size changes on geometry-matched lines: '+'; '.join(f'{aa or "(blank)"} -> {bb or "(blank)"} ({nn} lines)' for (aa,bb),nn in cc.items())+'.')
 else:text.append('  Cable sizes: no change detected on geometry-matched LV lines.')
 text.append(f"  Service cable geometric length: {fmt(s['old_length_m'])} -> {fmt(s['live_length_m'])} m; April-only route {fmt(s['removed_route_m'])} m; live-only route {fmt(s['added_route_m'])} m.")
 raw=x.get('live_designed_kva','');plan=x.get('live_kva','');old=x.get('old_kva','')
 text.append(f"  Transformer fields: April tr_size {old or '(blank)'}; live tr_size {raw or '(blank)'}; live plan_kva {plan or '(unset)'}. Nature: {x.get('live_nature') or '(unset)'}. Position movement: {fmt(x['transformer_move_m'] or 0)} m.")
 details={k:v for k,v in x['transformer_fields'].items() if k not in ('tr_size','name')}
 if details:text.append('  Other transformer attributes: '+'; '.join(f"{k}: {v['old'] or '(blank)'} -> {v['live'] or '(blank)'}" for k,v in details.items())+'.')
 text.append(f"  Calculated number-of-houses changed on {p['changed_fields'].get('num_houses',0)} matched poles.")
 text.append('  Customer attributes changed: '+(', '.join(f'{k} ({v})' for k,v in c['changed_fields'].items()) or 'none')+'.')
text+=['','SOURCE AND VERIFICATION',
'April input: C:\\Users\\nisun\\OneDrive\\Desktop\\GEOPACKAGE (file modification dates: 27 April 2026).',
'Live source: DS project czgmdwth_gisagara, stable; fresh fenced design.lv.project-export captures on 10 October 2026.',
'Initial membership verified against fresh transformer_scope tags. Source files verified against DS SHA-256 receipts.',
'Snapshot comparison only: changes are differences between the two datasets, not a reconstruction of their full edit history.']
(r/'Gisagara - GeoPackage comparison - initial scope.txt').write_text('\n'.join(text)+'\n')
print(json.dumps(summary,indent=2))

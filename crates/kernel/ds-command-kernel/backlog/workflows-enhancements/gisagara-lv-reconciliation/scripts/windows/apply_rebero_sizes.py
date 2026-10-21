import pathlib,json,subprocess,copy
r=pathlib.Path('C:/Users/nisun/ds-work/tmp/rebero-repair');r.mkdir(exist_ok=True);ds='C:/Users/nisun/AppData/Local/DS GridDesign/ds.exe';desc='C:/Users/nisun/AppData/Roaming/rw.datasolutions.desktop/cli-bridge.d/353da2ec3b22b49b7caf48d8fe61c2eb.json'
def call(a,n):
 p=subprocess.run([ds,*a,'--output','json'],capture_output=True,encoding='utf-8',timeout=240);j=json.loads(p.stdout);(r/(n+'.json')).write_text(json.dumps(j),encoding='utf-8');assert j['status']=='ok',j;return j['data']
def m(op,a,n):return call(['map','design',op,'--transformer','upgrade_rebero',*a,'--desktop-descriptor',desc],n)
source=call(['design','lv','project-export','--project','czgmdwth_gisagara','--transformer','upgrade_rebero','--out',str(r/'before.json')],'before-receipt');b=json.loads((r/'before.json').read_text(encoding='utf-8'))['jobs'][0]['gdfs'];expected=copy.deepcopy(b);ids=[]
for f in expected['lv_lines']['features']:
 if not f['properties'].get('cable_size'):ids.append(f['id']);f['properties']['cable_size']='3 x 35 + 54.6mm² ABC'
assert set(ids)==set(['line_10','line_11','line_13','line_14','line_22','line_3','line_5','line_6','line_8'])
m('open',[],'open');room=m('read',[],'room');assert not room['dirty'] and room['server_version']==source['source']['version']
a=['--layer','lv_lines','--where','cable_size=','--set','cable_size=3 x 35 + 54.6mm² ABC']
for fid in ids:a+=['--id',fid]
assert m('set',a+['--dry-run'],'preview')['matched']==9;assert m('set',a,'stage')['matched']==9;assert m('save',['--yes'],'save')['saved']
call(['design','lv','project-export','--project','czgmdwth_gisagara','--transformer','upgrade_rebero','--out',str(r/'after.json')],'after-receipt');after=json.loads((r/'after.json').read_text(encoding='utf-8'))['jobs'][0]['gdfs'];assert after==expected
print(json.dumps({'verified':True,'missing_sizes_restored':9,'only_property_changed':'lv_lines.cable_size','geometry_and_all_other_attributes_unchanged':True}))

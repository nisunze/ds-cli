import pathlib,json,subprocess,copy,collections
b=pathlib.Path(__file__).parent;ds='C:/Users/nisun/AppData/Local/DS GridDesign/ds.exe';desc='C:/Users/nisun/AppData/Roaming/rw.datasolutions.desktop/cli-bridge.d/353da2ec3b22b49b7caf48d8fe61c2eb.json';r=b/'survey-cable-final-proof';r.mkdir(exist_ok=True)
def call(a,n):
 p=subprocess.run([ds,*a,'--output','json'],capture_output=True,encoding='utf-8',timeout=240);j=json.loads(p.stdout);(r/(n+'.json')).write_text(p.stdout,encoding='utf-8');assert j['status']=='ok',j;return j['data']
n='fill_in_ep_gisanze'
def m(op,a,label):return call(['map','design',op,'--transformer',n,*a,'--desktop-descriptor',desc],label)
m('open',[],'open');assert not m('read',[],'room')['dirty']
for layer,fid in [('customers','customers_00000002'),('service_cables','service_cables_00000001')]:
 a=['--layer',layer,'--id',fid,'--set','drafting_status=approved'];assert m('set',a+['--dry-run'],layer+'-preview')['matched']==1;assert m('set',a,layer+'-stage')['matched']==1
assert m('save',['--yes'],'save')['saved']
def clean(f):
 f=copy.deepcopy(f);f.pop('id',None);f['properties'].pop('id',None);return json.dumps(f,sort_keys=True)
out=[]
for n,folder in [('fill_in_ep_gisanze','survey-cable-reconciliation-v2'),('kirwa','survey-cable-reconciliation-v3'),('ndaro','survey-cable-reconciliation-v4'),('rugarama','survey-cable-reconciliation-v4'),('tamba','survey-cable-reconciliation-v4')]:
 call(['design','lv','project-export','--project','czgmdwth_gisagara','--transformer',n,'--out',str(r/(n+'-after.json'))],n+'-receipt')
 g=json.loads((b/folder/n/'before.json').read_text())['jobs'][0]['gdfs'];a=json.loads((r/(n+'-after.json')).read_text())['jobs'][0]['gdfs']
 for layer,fc in g.items():
  old=collections.Counter(map(clean,fc['features']));new=collections.Counter(map(clean,a[layer]['features']));assert not(old-new),(n,layer)
  delta=sum((new-old).values());assert delta==(1 if layer=='customers' or (layer=='service_cables' and n=='fill_in_ep_gisanze') else 0),(n,layer,delta)
  for f in fc['features']:
   if f.get('id'):assert any(v.get('id')==f['id'] for v in a[layer]['features']),(n,layer,f['id'])
 added=[f for f in a['customers']['features'] if f['properties'].get('survey_form')=='edcl_customers_survey'];assert len(added)==1 and added[0]['properties']['drafting_status']=='approved'
 out.append({'transformer':n,'existing_customer_geometry_and_attributes_unchanged':True,'approved_sizes_unchanged':True,'one_survey_identified_customer_added':True,'survey_doc_id':added[0]['properties']['survey_doc_id'],'cable_restored':n=='fill_in_ep_gisanze','native_identity_normalization_only':True})
(r/'verified.json').write_text(json.dumps(out,indent=2),encoding='utf-8');print(json.dumps(out))

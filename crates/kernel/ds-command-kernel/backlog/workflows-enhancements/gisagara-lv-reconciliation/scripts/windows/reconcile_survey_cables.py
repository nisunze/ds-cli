import pathlib,json,subprocess,copy,math,collections
b=pathlib.Path(__file__).parent;r=b/'survey-cable-reconciliation-v4';r.mkdir(exist_ok=True)
ds='C:/Users/nisun/AppData/Local/DS GridDesign/ds.exe';desc='C:/Users/nisun/AppData/Roaming/rw.datasolutions.desktop/cli-bridge.d/353da2ec3b22b49b7caf48d8fe61c2eb.json'
def call(a,p):
 q=subprocess.run([ds,*a,'--output','json'],capture_output=True,encoding='utf-8',timeout=240);j=json.loads(q.stdout);p.write_text(q.stdout,encoding='utf-8');assert j['status']=='ok',j;return j['data']
def clean(f):
 f=copy.deepcopy(f)
 if f['properties'].get('id')==f.get('id'):f['properties'].pop('id',None)
 f.pop('id',None)
 return f
def canon(fc):return sorted(json.dumps(clean(f),sort_keys=True) for f in fc['features'])
def close(a,c):return math.hypot((a[0]-c[0])*111200*math.cos(math.radians(c[1])),(a[1]-c[1])*111200)<.25
plan=[x for x in json.loads((b/'service-review-with-april.json').read_text(encoding='utf-8')) if x['transformer'] in {'fill_in_ep_gisanze','kirwa','ndaro','tamba','rugarama'} and min(x['customer_distance_m'])>5 and x['transformer'] not in {'fill_in_ep_gisanze','kirwa'}]
for x in plan:
 n=x['transformer'];d=r/n;d.mkdir(exist_ok=True)
 source=call(['design','lv','project-export','--project','czgmdwth_gisagara','--transformer',n,'--out',str(d/'before.json')],d/'before-receipt.json');g=json.loads((d/'before.json').read_text())['jobs'][0]['gdfs']
 survey=json.loads((b/'orphan-cable-survey-check'/(n+'-full.json')).read_text())['data'];assert not survey['truncated']
 matches=[f for f in survey['entries'] if any(close(f['geometry']['coordinates'],c) for c in [x['geometry']['coordinates'][0],x['geometry']['coordinates'][-1]])];assert len(matches)==1
 s=matches[0];p=s['properties'];v=p['data'];sid=p['doc_id'];assert not any(close(f['geometry']['coordinates'],s['geometry']['coordinates']) for f in g['customers']['features'])
 props={'names':v.get('name',''),'category':v['category'],'transfo':g['tr']['features'][0]['properties'].get('transfo',n.upper()),'pole_number':x.get('pole_number',''),'drafting_status':'approved','survey_form':'edcl_customers_survey','survey_doc_id':sid}
 cust={'type':'Feature','id':sid,'geometry':s['geometry'],'properties':props};(d/'customer.geojson').write_text(json.dumps({'type':'FeatureCollection','features':[cust]}),encoding='utf-8')
 existing=[f for f in g['service_cables']['features'] if f['geometry']==x['geometry']]
 restore=n=='fill_in_ep_gisanze';assert len(existing)==(0 if restore else 1)
 cable=None
 if restore:
  old=json.loads((b/'orphan-service-removal'/n/'before.json').read_text())['jobs'][0]['gdfs']['service_cables']['features'];cable=[f for f in old if f['geometry']==x['geometry']];assert len(cable)==1;cable=cable[0];(d/'cable.geojson').write_text(json.dumps({'type':'FeatureCollection','features':[cable]}),encoding='utf-8')
 def m(op,a,label):return call(['map','design',op,'--transformer',n,*a,'--desktop-descriptor',desc],d/(label+'.json'))
 m('open',[],'open');room=m('read',[],'room');assert not room['dirty'] and room['server_version']==source['source']['version']
 batches=[('customers','customer.geojson',props)]
 if restore:batches.insert(0,('service_cables','cable.geojson',cable['properties']))
 for layer,path,pr in batches:
  a=['--features',str(d/path),'--target-layer',layer,'--set','drafting_status=approved']
  for k in pr:a+=['--carry-property',k]
  preview=m('create',a+['--dry-run'],layer+'-preview');print(n,layer,'preview',json.dumps(preview),flush=True)
  assert preview.get('accepted')==1 and not preview.get('rejected'),preview
  stage=m('create',a,layer+'-stage');assert stage.get('created')==1,stage
 assert m('save',['--yes'],'save')['saved']
 call(['design','lv','project-export','--project','czgmdwth_gisagara','--transformer',n,'--out',str(d/'after.json')],d/'after-receipt.json');after=json.loads((d/'after.json').read_text())['jobs'][0]['gdfs']
 for layer in g:
  if layer not in {'customers','service_cables'}:assert canon(after[layer])==canon(g[layer]),layer
 for layer in ['customers','service_cables']:
  old=collections.Counter(json.dumps(clean(f),sort_keys=True) for f in g[layer]['features']);new=collections.Counter(json.dumps(clean(f),sort_keys=True) for f in after[layer]['features']);assert not (old-new),layer
  assert sum((new-old).values())==(1 if layer=='customers' or restore else 0)
 added=[f for f in after['customers']['features'] if close(f['geometry']['coordinates'],s['geometry']['coordinates'])];assert len(added)==1
 for k,val in props.items():assert added[0]['properties'].get(k)==val,(k,added[0])
 (d/'verified.json').write_text(json.dumps({'verified':True,'survey_doc_id':sid,'customer_added':1,'cable_restored':restore,'prior_customers_unchanged':True,'unrelated_layers_and_sizes_unchanged':True}),encoding='utf-8');print(n,'verified',flush=True)




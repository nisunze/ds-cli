import pathlib,json,subprocess,time,collections,sys,os
from restore_verify import verify
base=pathlib.Path('C:/Users/nisun/ds-work/tmp/gpkg-comparison-20261010');plan=json.loads((base/'pole-restore-plan.json').read_text());work=base/'approved-restore';work.mkdir(exist_ok=True)
ds='C:/Users/nisun/AppData/Local/DS GridDesign/ds.exe';descriptor='C:/Users/nisun/AppData/Roaming/rw.datasolutions.desktop/cli-bridge.d/76b716223167f585285fd35f9f965a5b.json';project='czgmdwth_gisagara'
descriptor=os.environ.get('DS_RESTORE_DESCRIPTOR',descriptor)
def call(args,receipt):
 for attempt in range(4):
  r=subprocess.run([ds,*args,'--output','json'],capture_output=True,encoding='utf-8',timeout=240)
  try:j=json.loads(r.stdout)
  except Exception:raise RuntimeError(str(args[:4])+': invalid response '+r.stdout[:200])
  receipt.write_text(json.dumps(j,indent=2),encoding='utf-8')
  if j['status']=='ok':return j['data']
  retryable=j.get('error',{}).get('code') in ('native_state_conflict','auth_transient') or (args[:3]==['map','design','open'] and j.get('error',{}).get('code')=='desktop_unreadable' and j.get('error',{}).get('retryable'))
  if retryable and attempt<3:time.sleep(2+attempt);continue
  raise RuntimeError(str(args[:4])+': '+json.dumps(j.get('error')))
def mapcall(name,op,args,receipt):return call(['map','design',op,'--transformer',name,*args,'--desktop-descriptor',descriptor],receipt)
def export(name,prefix):return call(['design','lv','project-export','--project',project,'--transformer',name,'--out',str(prefix.with_suffix('.json'))],prefix.with_suffix('.receipt.json'))
def layers(path):return json.loads(path.read_text())['jobs'][0]['gdfs']
def byid(data):
 return {k:{str(f['id']):f for f in v['features']} for k,v in data.items()}
def apply(p):
 name=p['transformer'];folder=work/name;folder.mkdir(exist_ok=True);done=folder/'verified.json'
 if done.exists():print(name,'already verified',flush=True);return
 if (folder/'after.json').exists():
  proof=verify(p,folder);print(name,'VERIFIED resumed',proof['restored_poles'],flush=True);return
 before=folder/'before'
 if not before.with_suffix('.json').exists():source=export(name,before)
 else:source=json.loads(before.with_suffix('.receipt.json').read_text())['data']
 current=layers(before.with_suffix('.json'));original=layers(base/'live'/(name+'.json'))
 assert current==original,'Live head changed since comparison: '+name
 opened=mapcall(name,'open',[],folder/'open.json');assert opened['project']==project and opened['transformer']==name and not opened['dirty']
 room=mapcall(name,'read',[],folder/'room.json');assert not room['dirty'] and room['server_version']==source['source']['version'],(name,'local/server head mismatch')
 expected_counts={k:len(v['features']) for k,v in current.items()};assert room['layers']==expected_counts,(name,'layer count mismatch')
 poles=mapcall(name,'select',['--layer','lv_poles','--ids','5000'],folder/'pole-ids.json');assert set(poles['ids'])==set(str(x['id']) for x in current['lv_poles']['features'])
 groups=collections.defaultdict(list)
 for x in p['restore_changes']:groups[(x['old_current'],x['restore'])].append(x['id'])
 edits=[]
 for i,((old,new),ids) in enumerate(sorted(groups.items())):
  args=['--layer','lv_poles','--where','struct_type='+old,'--set','struct_type='+new]
  for featureid in ids:args+=['--id',featureid]
  dry=mapcall(name,'set',[*args,'--dry-run'],folder/f'type-{i}-preview.json');assert dry['matched']==len(ids),(name,dry,len(ids))
  edits.append((i,args,len(ids)))
 dry=mapcall(name,'set',['--set','drafting_status=approved','--dry-run'],folder/'approval-preview.json');assert dry['matched']==sum(expected_counts.values()),(name,dry)
 for i,args,n in edits:
  applied=mapcall(name,'set',args,folder/f'type-{i}-staged.json');assert applied['matched']==n
 approval=mapcall(name,'set',['--set','drafting_status=approved'],folder/'approval-staged.json');assert approval['matched']==sum(expected_counts.values())
 approved=mapcall(name,'select',['--where','drafting_status=approved'],folder/'approval-check.json');assert approved['matched']==sum(expected_counts.values())
 for i,((old,new),ids) in enumerate(sorted(groups.items())):
  args=['--layer','lv_poles','--where','struct_type='+new]
  for featureid in ids:args+=['--id',featureid]
  checked=mapcall(name,'select',args,folder/f'type-{i}-check.json');assert checked['matched']==len(ids)
 saved=mapcall(name,'save',['--yes'],folder/'saved.json');assert saved.get('saved'),saved
 after=folder/'after';readback=export(name,after)
 # Feature order is not an authored fact; every feature, geometry and other property is.
 proof=verify(p,folder)
 proof={'transformer':name,'restored_poles':len(p['restore_changes']),'approved_features':sum(expected_counts.values()),'layer_counts':expected_counts,'source':source['source'],'saved':readback['source'],'only_changed_properties':['struct_type','drafting_status'],'geometry_unchanged':True,'counts_unchanged':True}
 print(name,'VERIFIED restored',len(p['restore_changes']),'approved',sum(expected_counts.values()),flush=True)
names=sys.argv[1:];selected=[p for p in plan['transformers'] if not names or p['transformer'] in names]
if os.environ.get('DS_RESTORE_REVERSE')=='1':selected.reverse()
for i,p in enumerate(selected,1):
 print('START',i,len(selected),p['transformer'],flush=True);apply(p)
print('All selected transformers verified.',flush=True)

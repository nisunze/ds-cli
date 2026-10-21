import pathlib,json,sys,collections,math
b=pathlib.Path('/home/magese/projects/gisagara');sys.path.insert(0,str(b/'gpkg-comparison-20261010/python-libs'))
from shapely.geometry import shape,Point
from shapely.ops import transform
from pyproj import Transformer
from scipy.spatial import cKDTree
r=b/'refreshed87-20261010';q=json.loads((r/'request.json').read_text());proj=Transformer.from_crs(4326,32735,always_xy=True).transform;rows=[];globalcustomers=collections.defaultdict(list);globalpoles=collections.defaultdict(list)
def near(tree,p):return float(tree.query(p)[0]) if tree is not None else float('inf')
for item in q['transformers']:
 n=item['transformer'];raw=json.loads((r/'inputs'/(n+'.json')).read_text())['jobs'][0]['gdfs'];gs={};issues=[];summary=collections.Counter()
 def issue(code,layer,f,**extra):
  summary[code]+=1;issues.append({'code':code,'layer':layer,'id':f.get('id'),'pole_number':f.get('properties',{}).get('pole_number'),**extra})
 for layer,fc in raw.items():
  fs=[];ids=collections.Counter(str(f['id']) for f in fc['features'] if 'id' in f)
  for fid,count in ids.items():
   if count>1:summary['duplicate_feature_id']+=count-1;issues.append({'code':'duplicate_feature_id','layer':layer,'id':fid,'count':count})
  geoms=collections.defaultdict(list)
  for f in fc['features']:
   try:g=transform(proj,shape(f['geometry']))
   except Exception:issue('unreadable_geometry',layer,f);continue
   if g.is_empty or not g.is_valid:issue('invalid_geometry',layer,f)
   if layer in ['lv_lines','spans','service_cables'] and g.length<.01:issue('zero_length_line',layer,f,length_m=g.length)
   geoms[g.normalize().wkb_hex].append(f.get('id'))
   if layer in ['lv_lines','spans','service_cables']:
    if not f['properties'].get('cable_size') and not f['properties'].get('cab_size'):issue('missing_cable_size',layer,f)
    v=f['properties'].get('length')
    if isinstance(v,(int,float)) and abs(v-g.length)>max(1,g.length*.02):issue('stored_length_differs',layer,f,stored_m=v,geometry_m=round(g.length,3))
   fs.append((f,g))
  for ids0 in geoms.values():
   if len(ids0)>1:summary['duplicate_geometry']+=len(ids0)-1;issues.append({'code':'duplicate_geometry','layer':layer,'ids':ids0})
  gs[layer]=fs
 customers=gs.get('customers',[]);poles=gs.get('lv_poles',[])+gs.get('tapping_poles',[])+gs.get('tr',[])
 cpoints=[(g.x,g.y) for f,g in customers if g.geom_type=='Point'];ppoints=[(g.x,g.y) for f,g in poles if g.geom_type=='Point'];ct=cKDTree(cpoints) if cpoints else None;pt=cKDTree(ppoints) if ppoints else None
 linked=set();adj=collections.defaultdict(set)
 def node(p):return tuple(round(v,3) for v in p)
 for f,g in gs.get('spans',[]):
  if g.geom_type!='LineString':issue('unexpected_span_geometry', 'spans',f);continue
  a,z=g.coords[0],g.coords[-1];adj[node(a)].add(node(z));adj[node(z)].add(node(a))
  for p in [a,z]:
   if near(pt,p)>.25:issue('span_endpoint_without_pole_or_transformer','spans',f,distance_m=round(near(pt,p),3))
 starts=[node((g.x,g.y)) for f,g in gs.get('tr',[])+gs.get('tapping_poles',[]) if g.geom_type=='Point'];seen=set(starts);todo=list(starts)
 while todo:
  for v in adj[todo.pop()]:
   if v not in seen:seen.add(v);todo.append(v)
 for f,g in gs.get('lv_poles',[]):
  if g.geom_type=='Point' and node((g.x,g.y)) not in seen:issue('pole_not_reachable_in_exact_span_graph','lv_poles',f)
 for f,g in gs.get('service_cables',[]):
  if g.geom_type!='LineString':issue('unexpected_service_geometry','service_cables',f);continue
  ends=[g.coords[0],g.coords[-1]];cm=[near(ct,p) for p in ends];pm=[near(pt,p) for p in ends];valid=[]
  for ci,pi in [(0,1),(1,0)]:
   if cm[ci]<=.25 and pm[pi]<=.25:valid.append((ci,pi))
  if not valid:issue('service_endpoint_assignment_candidate','service_cables',f,customer_distances_m=[round(x,3) for x in cm],pole_distances_m=[round(x,3) for x in pm])
  else:
   ci,pi=valid[0];customeridx=int(ct.query(ends[ci])[1]);linked.add(customeridx)
   if node(ends[pi]) not in seen:issue('service_source_not_reachable_in_exact_span_graph','service_cables',f)
 for i,(f,g) in enumerate(customers):
  if i not in linked:issue('customer_without_geometric_service_match','customers',f)
  if g.geom_type=='Point':globalcustomers[node((g.x,g.y))].append((n,f.get('id')))
 for f,g in gs.get('lv_poles',[]):
  if g.geom_type=='Point':globalpoles[node((g.x,g.y))].append((n,f.get('id')))
 rows.append({'transformer':n,'counts':{k:len(v) for k,v in gs.items()},'summary':dict(summary),'issues':issues})
shared={'customers':[{'xy':k,'features':v} for k,v in globalcustomers.items() if len({x[0] for x in v})>1],'poles':[{'xy':k,'features':v} for k,v in globalpoles.items() if len({x[0] for x in v})>1]}
result={'scope':87,'method':'Read-only external diagnostic of DS exports; metric EPSG32735; endpoint tolerance0.25m; exact graph normalized0.001m; findings are review candidates, not automatic engineering verdicts','transformers':rows,'cross_transformer_duplicates':shared};(r/'integrity-audit.json').write_text(json.dumps(result,indent=2));total=collections.Counter()
for x in rows:total.update(x['summary'])
print('TOTAL',json.dumps(total));print('SHARED', {k:len(v) for k,v in shared.items()});print('SITES',json.dumps([{k:v for k,v in x.items() if k in ['transformer','summary']} for x in rows if x['summary']]))


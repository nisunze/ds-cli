import sys,pathlib,json,sqlite3,zipfile,collections,math,hashlib
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');sys.path.insert(0,str(r/'python-libs'))
from shapely import wkb
from shapely.geometry import shape
from shapely.ops import transform,unary_union
from pyproj import Transformer
from scipy.spatial import cKDTree
with zipfile.ZipFile(r/'live-inputs.zip') as z:z.extractall(r)
scope=json.loads((r/'scope87.json').read_text(encoding='utf-8-sig'))['data']['grouping']['groups'][0]['transformers'];assert len(scope)==87
proj=Transformer.from_crs(4326,32735,always_xy=True).transform
def geom(blob):
 flags=blob[3];env=(flags>>1)&7;offset=8+{0:0,1:32,2:48,3:48,4:64}[env]
 return transform(proj,wkb.loads(blob[offset:]))
def oldread(f):
 c=sqlite3.connect('file:'+str(f)+'?mode=ro',uri=True);c.row_factory=sqlite3.Row;out={}
 for t, in c.execute("select table_name from gpkg_contents where data_type='features'"):
  out[t]=[{'properties':{k:v for k,v in dict(row).items() if k not in ('fid','geom')},'g':geom(row['geom'])} for row in c.execute('select * from "'+t.replace('"','""')+'"')]
 c.close();return out
def live(f):
 j=json.loads(f.read_text());out={}
 for k,v in j['jobs'][0]['gdfs'].items():out[k]=[{'properties':x.get('properties',{}),'g':transform(proj,shape(x['geometry']))} for x in v.get('features',[]) if x.get('geometry')]
 return out
def norm(v):
 if v is None or str(v).strip().lower() in ('none','null','nan',''):return ''
 if isinstance(v,(dict,list)):return json.dumps(v,sort_keys=True)
 s=str(v).replace('Â','').strip()
 try:return str(round(float(s),3))
 except ValueError:return s
def pointcompare(a,b,fields,key):
 pairs=[];used=set();unused=set(range(len(a)))
 if b:
  tree=cKDTree([(x['g'].x,x['g'].y) for x in b]);edges=[]
  for i,x in enumerate(a):
   for j in tree.query_ball_point([x['g'].x,x['g'].y],.25):edges.append((x['g'].distance(b[j]['g']),i,j))
  for d,i,j in sorted(edges):
   if i in unused and j not in used:pairs.append((i,j,d));unused.remove(i);used.add(j)
 # A moved point is only paired by an unambiguous preserved label within 100 m.
 am=collections.defaultdict(list);bm=collections.defaultdict(list)
 for i in unused:am[norm(a[i]['properties'].get(key))].append(i)
 for j in range(len(b)):
  if j not in used:bm[norm(b[j]['properties'].get(key))].append(j)
 for label,ii in am.items():
  jj=bm.get(label,[])
  if label and len(ii)==len(jj)==1:
   i,j=ii[0],jj[0];d=a[i]['g'].distance(b[j]['g'])
   if d<=100:pairs.append((i,j,d));unused.remove(i);used.add(j)
 changes=collections.Counter();examples=[];moved=0
 for i,j,d in pairs:
  if d>.25:moved+=1
  for field in fields:
   if field not in a[i]['properties'] or field not in b[j]['properties']:continue
   x,y=norm(a[i]['properties'][field]),norm(b[j]['properties'][field])
   if x!=y:
    changes[field]+=1
    if len(examples)<30:examples.append({'label_old':a[i]['properties'].get(key),'label_live':b[j]['properties'].get(key),'field':field,'old':x,'live':y})
 return {'old':len(a),'live':len(b),'removed_or_unmatched':len(unused),'added_or_unmatched':len(b)-len(used),'moved_label_matches':moved,'matched':len(pairs),'changed_fields':dict(changes),'examples':examples}
def lines(a,b):
 ga=unary_union([x['g'] for x in a]);gb=unary_union([x['g'] for x in b]);oldlen=ga.length;newlen=gb.length
 # Coverage tests ignore line segmentation and direction. A 0.5 m buffer absorbs coordinate precision only.
 removed=ga.difference(gb.buffer(.5)).length if b else oldlen;added=gb.difference(ga.buffer(.5)).length if a else newlen
 def size_lengths(v):
  d=collections.Counter()
  for x in v:d[norm(x['properties'].get('cable_size',''))]+=x['g'].length
  return {k:round(n,2) for k,n in sorted(d.items())}
 return {'old_count':len(a),'live_count':len(b),'old_length_m':round(sum(x['g'].length for x in a),2),'live_length_m':round(sum(x['g'].length for x in b),2),'removed_route_m':round(removed,2),'added_route_m':round(added,2),'old_cable_m':size_lengths(a),'live_cable_m':size_lengths(b)}
out=[]
for name in scope:
 f=r/'gpkg'/(name+'.gpkg');now=live(r/'live'/(name+'.json'));row={'transformer':name}
 if not f.exists():row['baseline']='No April GeoPackage';out.append(row);continue
 old=oldread(f);row['baseline']='Matched';row['counts']={k:[len(old.get(k,[])),len(now.get(k,[]))] for k in ('tr','lv_poles','lv_lines','customers','service_cables','spans')}
 row['poles']=pointcompare(old.get('lv_poles',[]),now.get('lv_poles',[]),['pole_number','cable_size','earthing','stay','struct_type','assembly_type','position','num_houses'],'pole_number')
 row['customers']=pointcompare(old.get('customers',[]),now.get('customers',[]),['pole_number','house_number','category','meter_type','service_length'],'house_number')
 row['lv_lines']=lines(old.get('lv_lines',[]),now.get('lv_lines',[]));row['service_cables']=lines(old.get('service_cables',[]),now.get('service_cables',[]))
 row['pole_types']={s:dict(collections.Counter(norm(x['properties'].get('struct_type')) for x in data.get('lv_poles',[]))) for s,data in [('old',old),('live',now)]}
 row['line_cable_changes']=[]
 usedlines=set()
 from shapely.strtree import STRtree
 bl=now.get('lv_lines',[])
 if bl:
  tree=STRtree([x['g'] for x in bl])
  for x in old.get('lv_lines',[]):
   candidates=[(x['g'].hausdorff_distance(bl[int(j)]['g']),int(j)) for j in tree.query(x['g'].buffer(.25)) if int(j) not in usedlines]
   if candidates:
    d,j=min(candidates)
    if d<=.25:
     usedlines.add(j);a,b=norm(x['properties'].get('cable_size')),norm(bl[j]['properties'].get('cable_size'))
     if a!=b:row['line_cable_changes'].append({'old_line':x['properties'].get('line_number'),'live_line':bl[j]['properties'].get('line_number'),'old_cable':a,'live_cable':b,'length_m':round(x['g'].length,2)})
 ot=old.get('tr',[]);nt=now.get('tr',[]);row['transformer_fields']={};row['transformer_move_m']=None
 if len(ot)==len(nt)==1:
  row['transformer_move_m']=round(ot[0]['g'].distance(nt[0]['g']),3)
  for k in ['tr_size','ex_tr_size','main_cable','standard_c','lv_circuit','mv_fuze_30','name']:
   nk={'main_cable':'main_cable_size','standard_c':'standard_cable_transfo_to_db','lv_circuit':'lv_circuit_breaker','mv_fuze_30':'mv_fuze_30_kv_amp'}.get(k,k)
   a,b=norm(ot[0]['properties'].get(k)),norm(nt[0]['properties'].get(nk))
   if a!=b:row['transformer_fields'][nk]={'old':a,'live':b}
  row['old_kva']=norm(ot[0]['properties'].get('tr_size'));row['live_kva']=norm(nt[0]['properties'].get('plan_kva',nt[0]['properties'].get('tr_size')));row['live_designed_kva']=norm(nt[0]['properties'].get('tr_size'));row['live_nature']=nt[0]['properties'].get('transformer_nature')
 out.append(row)
(r/'comparison.json').write_text(json.dumps(out,indent=2))
print('Compared',len(out),'missing',[x['transformer'] for x in out if x['baseline']!='Matched'])
for x in out:
 if x['baseline']!='Matched':print(x['transformer'],'NO BASELINE');continue
 p=x['poles'];c=x['customers'];l=x['lv_lines'];print(x['transformer'],'poles',p['old'],p['live'],'(-/+)',p['removed_or_unmatched'],p['added_or_unmatched'],'fields',p['changed_fields'],'customers',c['old'],c['live'],'route(-/+m)',l['removed_route_m'],l['added_route_m'],'kVA',x.get('old_kva'),x.get('live_kva'))


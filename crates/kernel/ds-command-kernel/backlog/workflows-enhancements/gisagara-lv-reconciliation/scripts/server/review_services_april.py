import pathlib,json,collections
b=pathlib.Path('/home/magese/projects/gisagara');ns={};exec((b/'gpkg-comparison-20261010/gpkg_compare.py').read_text().split('out=[]')[0],ns);r=b/'refreshed87-20261010';from scipy.spatial import cKDTree
native_orphans={'fill_in_ep_gisanze','kirwa','ndaro','tamba'};out=[]
for item in json.loads((r/'request.json').read_text())['transformers']:
 n=item['transformer'];g=ns['live'](r/'inputs'/(n+'.json'));raw=json.loads((r/'inputs'/(n+'.json')).read_text())['jobs'][0]['gdfs'];oldpath=b/'gpkg-comparison-20261010/gpkg'/(n+'.gpkg');old=ns['oldread'](oldpath) if oldpath.exists() else {};cs=g.get('customers',[]);ps=g.get('lv_poles',[])+g.get('tapping_poles',[])+g.get('tr',[]);ct=cKDTree([(x['g'].x,x['g'].y) for x in cs]) if cs else None;pt=cKDTree([(x['g'].x,x['g'].y) for x in ps]) if ps else None
 if ct is None or pt is None:continue
 for i,f in enumerate(g.get('service_cables',[])):
  e=[f['g'].coords[0],f['g'].coords[-1]];cd=[float(ct.query(p)[0]) for p in e];pd=[float(pt.query(p)[0]) for p in e];valid=any(cd[a]<=.25 and pd[z]<=.25 for a,z in [(0,1),(1,0)])
  if valid:continue
  baseline=[x for x in old.get('service_cables',[]) if f['g'].hausdorff_distance(x['g'])<=.001 and abs(f['g'].length-x['g'].length)<=.001];oc=old.get('customers',[]);old_near=min((x['g'].distance(__import__('shapely').geometry.Point(p)) for x in oc for p in e),default=None)
  prop=f['properties'];out.append({'transformer':n,'selector':raw['service_cables']['features'][i].get('id','service_cables#'+str(i)),'pole_number':prop.get('pole_number'),'geometry':raw['service_cables']['features'][i]['geometry'],'customer_distance_m':cd,'pole_distance_m':pd,'april_exact_cable_matches':len(baseline),'nearest_april_customer_to_either_endpoint_m':old_near,'properties':prop,'native_orphan_confirmed':n in native_orphans and min(cd)>5})
(r/'service-review-with-april.json').write_text(json.dumps(out,indent=2));print(json.dumps([{k:v for k,v in x.items() if k!='geometry'} for x in out]))


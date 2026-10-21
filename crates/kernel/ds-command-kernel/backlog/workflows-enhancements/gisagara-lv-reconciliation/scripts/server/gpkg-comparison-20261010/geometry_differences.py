import pathlib,json,sys,sqlite3,struct,datetime,collections
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');sys.path.insert(0,str(r/'python-libs'))
exec((r/'gpkg_compare.py').read_text().split('out=[]')[0])
from shapely.geometry import mapping
from shapely import to_wkb
from shapely.strtree import STRtree
inv=Transformer.from_crs(32735,4326,always_xy=True).transform
o=r/'geometry-differences';o.mkdir(exist_ok=True);sets={s:{'points':[],'lines':[]} for s in ['additions','removals']};changed=set();counts={}
excluded={'zihare','ruhuha','upgrade_mbeho','substation'}
def emit(side,n,layer,f,g):
 if g.is_empty:return
 if g.geom_type in ['MultiLineString','GeometryCollection','MultiPoint']:
  for sub in g.geoms:emit(side,n,layer,f,sub)
  return
 if g.geom_type not in ['Point','LineString']:return
 if g.geom_type=='LineString' and g.length<.1:return
 props=f['properties'];p={'transformer':n,'source_layer':layer,'change':side,'source_id':str(props.get('id',props.get('pole_number',props.get('house_number','')))),'length_m':round(g.length,3) if g.geom_type=='LineString' else None}
 sets[side]['points' if g.geom_type=='Point' else 'lines'].append({'type':'Feature','geometry':mapping(transform(inv,g)),'properties':p});changed.add(n)
for n in scope:
 if n in excluded:continue
 old=oldread(r/'gpkg'/(n+'.gpkg'));new=live(r/'restoration/approved-restore'/n/'after.json')
 for layer in ['tr','lv_poles','tapping_poles','customers','lv_lines','service_cables','spans']:
  a=old.get(layer,[]);b=new.get(layer,[])
  for side,src,base in [('additions',b,a),('removals',a,b)]:
   pts=[x['g'] for x in base if x['g'].geom_type=='Point'];tree=STRtree(pts) if pts else None
   ls=[x['g'] for x in base if 'LineString' in x['g'].geom_type];cover=unary_union(ls).buffer(.01) if ls else None
   for f in src:
    g=f['g']
    if g.geom_type=='Point':
     if tree is None or not any(g.distance(pts[int(i)])<=.001 for i in tree.query(g.buffer(.001))):emit(side,n,layer,f,g)
    elif 'LineString' in g.geom_type:emit(side,n,layer,f,g.difference(cover) if cover is not None else g)
def gpkg(path,data):
 if path.exists():path.unlink()
 c=sqlite3.connect(path);c.execute('PRAGMA application_id=1196444487');c.execute('PRAGMA user_version=10300')
 c.executescript('CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT NOT NULL,srs_id INTEGER NOT NULL PRIMARY KEY,organization TEXT NOT NULL,organization_coordsys_id INTEGER NOT NULL,definition TEXT NOT NULL,description TEXT);CREATE TABLE gpkg_contents (table_name TEXT NOT NULL PRIMARY KEY,data_type TEXT NOT NULL,identifier TEXT UNIQUE,description TEXT DEFAULT "",last_change DATETIME NOT NULL,min_x DOUBLE,min_y DOUBLE,max_x DOUBLE,max_y DOUBLE,srs_id INTEGER);CREATE TABLE gpkg_geometry_columns (table_name TEXT NOT NULL,column_name TEXT NOT NULL,geometry_type_name TEXT NOT NULL,srs_id INTEGER NOT NULL,z TINYINT NOT NULL,m TINYINT NOT NULL,PRIMARY KEY(table_name,column_name));')
 from pyproj import CRS
 for sid,label,org in [(-1,'Undefined Cartesian','NONE'),(0,'Undefined geographic','NONE'),(4326,'WGS 84','EPSG')]:c.execute('INSERT INTO gpkg_spatial_ref_sys VALUES (?,?,?,?,?,?)',(label,sid,org,sid,CRS.from_epsg(4326).to_wkt(version='WKT1_GDAL') if sid==4326 else 'undefined',''))
 for table,features in data.items():
  c.execute(f'CREATE TABLE {table} (fid INTEGER PRIMARY KEY AUTOINCREMENT,geom BLOB,transformer TEXT,source_layer TEXT,change TEXT,source_id TEXT,length_m REAL)')
  bb=unary_union([shape(f['geometry']) for f in features]).bounds
  c.execute('INSERT INTO gpkg_contents VALUES (?,?,?,?,?,?,?,?,?,?)',(table,'features',table,'Geometric differences only',datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ'),*bb,4326))
  c.execute('INSERT INTO gpkg_geometry_columns VALUES (?,?,?,?,?,?)',(table,'geom','POINT' if table=='points' else 'LINESTRING',4326,0,0))
  for f in features:
   g=shape(f['geometry']);blob=b'GP'+bytes([0,1])+struct.pack('<i',4326)+to_wkb(g,byte_order=1);p=f['properties'];c.execute(f'INSERT INTO {table} (geom,transformer,source_layer,change,source_id,length_m) VALUES (?,?,?,?,?,?)',(blob,p['transformer'],p['source_layer'],p['change'],p['source_id'],p['length_m']))
 c.commit();assert c.execute('PRAGMA integrity_check').fetchone()[0]=='ok';c.close()
for side,data in sets.items():
 gpkg(o/('Gisagara-'+side+'.gpkg'),data)
 for kind,features in data.items():(o/(side+'-'+kind+'.geojson')).write_text(json.dumps({'type':'FeatureCollection','features':features}))
 counts[side]={k:len(v) for k,v in data.items()}
proof={'counts':counts,'changed_transformers':sorted(changed),'excluded':sorted(excluded),'point_tolerance_m':.001,'line_coverage_tolerance_m':.01,'minimum_line_fragment_m':.1,'comparison':'April baseline against post-restoration saved designs; geometry only; moved positions appear as removal plus addition'}
(o/'geometry-difference-proof.json').write_text(json.dumps(proof,indent=2));print(json.dumps(proof,indent=2))

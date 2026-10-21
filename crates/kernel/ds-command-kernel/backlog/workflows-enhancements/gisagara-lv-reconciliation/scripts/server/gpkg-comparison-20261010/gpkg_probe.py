import pathlib,json,sqlite3,zipfile
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');r.mkdir(exist_ok=True)
with zipfile.ZipFile(r/'old-inputs.zip') as z:z.extractall(r)
try:
 import geopandas,shapely,pyproj
 print('Libraries',geopandas.__version__,shapely.__version__)
except ImportError as e:print(e)
f=r/'gpkg/agahehe.gpkg';c=sqlite3.connect('file:'+str(f)+'?mode=ro',uri=True)
tables=c.execute('select table_name,data_type,srs_id from gpkg_contents').fetchall();print('TABLES',tables)
for t,typ,srs in tables:
 cols=c.execute('pragma table_info("'+t.replace('"','""')+'")').fetchall()
 print(t,'COUNT',c.execute('select count(*) from "'+t.replace('"','""')+'"').fetchone()[0],'COLS',[x[1] for x in cols])
 row=c.execute('select * from "'+t.replace('"','""')+'" limit 1').fetchone()
 if t in ['tr','transformer','transformers'] or 'trans' in t.lower():print('TR SAMPLE',{cols[i][1]:v for i,v in enumerate(row) if not isinstance(v,bytes)})

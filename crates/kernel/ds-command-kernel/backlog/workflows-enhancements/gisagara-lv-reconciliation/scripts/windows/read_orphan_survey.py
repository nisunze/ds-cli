import pathlib,json,subprocess,math
b=pathlib.Path(__file__).parent;r=b/'orphan-cable-survey-check';ds='C:/Users/nisun/AppData/Local/DS GridDesign/ds.exe'
plan=json.loads((b/'service-review-with-april.json').read_text(encoding='utf-8'))
for x in plan:
 if x['transformer'] not in {'fill_in_ep_gisanze','kirwa','ndaro','tamba','rugarama'} or min(x['customer_distance_m'])<=5:continue
 cs=x['geometry']['coordinates'];sel=json.loads((r/(x['transformer']+'.json')).read_text())['data'];rows=sel['rows'];matches=[]
 for row in rows:
  p=row['geometry']['coordinates'];dist=min(math.hypot((p[0]-v[0])*111200*math.cos(math.radians(p[1])),(p[1]-v[1])*111200) for v in [cs[0],cs[-1]])
  if dist<.25:matches.append({'doc_id':row['doc_id'],'distance_m':dist})
 print(x['transformer'],'survey_endpoint_matches',matches,flush=True)
 bbox=','.join(str(v) for v in sel['bbox']) if isinstance(sel['bbox'],list) else sel['bbox']
 if not isinstance(bbox,str):
  xs=[p[0] for p in cs];ys=[p[1] for p in cs];bbox=','.join(map(str,[min(xs)-.00025,min(ys)-.00025,max(xs)+.00025,max(ys)+.00025]))
 p=subprocess.run([ds,'survey','entries','read','--project','czgmdwth_gisagara','--form','edcl_customers_survey','--bbox',bbox,'--limit','500','--output','json'],capture_output=True,encoding='utf-8',timeout=180)
 j=json.loads(p.stdout);(r/(x['transformer']+'-full.json')).write_text(p.stdout,encoding='utf-8');assert j['status']=='ok',j
 print('full_read',x['transformer'],'keys',list(j['data']),flush=True)

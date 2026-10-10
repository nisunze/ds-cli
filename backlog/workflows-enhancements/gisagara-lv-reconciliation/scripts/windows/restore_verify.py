import json
def verify(p,folder):
 name=p['transformer'];source=json.loads((folder/'before.receipt.json').read_text())['data'];readback=json.loads((folder/'after.receipt.json').read_text())['data']
 current=json.loads((folder/'before.json').read_text())['jobs'][0]['gdfs'];actual=json.loads((folder/'after.json').read_text())['jobs'][0]['gdfs'];expected=json.loads(json.dumps(current))
 def byid(data):return {k:{('id:'+str(f['id']) if 'id' in f else 'row:'+str(i)):f for i,f in enumerate(v['features'])} for k,v in data.items()}
 ei=byid(expected);ai=byid(actual);assert ei.keys()==ai.keys();filled=0
 for layer,features in ei.items():
  assert features.keys()==ai[layer].keys()
  for fid,f in features.items():
   props=f.setdefault('properties',{});props['drafting_status']='approved';ap=ai[layer][fid]['properties']
   # Desktop save fills the owning transformer reference if absent. It must
   # name this same transformer and cannot rewrite an authored reference.
   if 'transfo' not in props and 'transfo' in ap:
    assert ap['transfo'].casefold()==name.casefold();props['transfo']=ap['transfo'];filled+=1
 for x in p['restore_changes']:ei['lv_poles']['id:'+x['id']]['properties']['struct_type']=x['restore']
 assert ai==ei,'Unexpected saved-data difference at '+name
 proof={'transformer':name,'restored_poles':len(p['restore_changes']),'approved_features':sum(len(v['features']) for v in current.values()),'layer_counts':{k:len(v['features']) for k,v in current.items()},'source':source['source'],'saved':readback['source'],'authored_changed_properties':['struct_type','drafting_status'],'save_filled_same_transformer_references':filled,'geometry_unchanged':True,'counts_unchanged':True}
 (folder/'verified.json').write_text(json.dumps(proof,indent=2));return proof

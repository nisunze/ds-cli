import pathlib,json,collections
r=pathlib.Path('/home/magese/projects/gisagara/refreshed87-20261010');j=json.loads((r/'native-integrity-review.json').read_text());tot=collections.Counter()
for x in j:
 c=collections.Counter()
 for w in x['warnings']:c[w['code']]+=w.get('count',1)
 tot.update(c);print(x['transformer'],json.dumps(c),'protected',x['protected_change_candidates'][:8])
print('TOTAL',json.dumps(tot))

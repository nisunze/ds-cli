from pathlib import Path
import json,zipfile,hashlib,xml.etree.ElementTree as E,openpyxl,math,re
r=Path('/home/magese/projects/gisagara');o=r/'final-aligned-20261010';q=json.loads((o/'request.json').read_text());trs={x['transformer']:json.loads(Path(x['layers']).read_text())['layers']['tr']['features'][0]['properties'] for x in q['transformers']}
src=r/'staking-without-gatovu-20261010/Stacking Table rev3 - 20 L-susp to A - 2026-10-09.xlsx'
w=openpyxl.load_workbook(src,data_only=True);s=w.active
cols={int(re.match(r'(\d+)',str(s.cell(2,c).value)).group(1)):c for c in range(40,48)}
lv=openpyxl.load_workbook(o/'retained-supply-native/combined_transformer.xlsx',data_only=True)['LV Summary'];lc={int(re.match(r'(\d+)',str(lv.cell(3,c).value)).group(1)):c for c in range(1,lv.max_column+1) if re.fullmatch(r'\d+\s*kVA',str(lv.cell(3,c).value))}
items={str(lv.cell(i,1).value).lower():{k:lv.cell(i,c).value or 0 for k,c in lc.items()} for i in range(5,lv.max_row+1) if lv.cell(i,1).value and lv.cell(i,1).value!='Total'}
matched=[];tot={k:0 for k in lc}
for i in range(3,429):
 vals={k:s.cell(i,c).value or 0 for k,c in cols.items()};supply=sum(vals.values())
 if not supply:continue
 xy=[s.cell(i,c).value for c in (3,4)]
 candidates=sorted((math.hypot(float(p['x'])-xy[0],float(p['y'])-xy[1]),n) for n,p in trs.items() if p.get('x') and p.get('y'))
 dist,n=candidates[0];assert dist<25,(i,s.cell(i,12).value,dist,n)
 assert {k:vals.get(k,0) for k in lc}==items[n],(i,n,vals,items[n])
 for k in lc:tot[k]+=vals.get(k,0)
 matched.append({'row':i,'site':n,'distance_m':dist,'supply':supply})
assert len(matched)==64 and len({x['site'] for x in matched})==64
p=trs['substation'];assert p['tr_size']==100 and items['substation'][100]==1
assert sum(items['kirwa'].values())==0
assert {k:tot[k]+(1 if k==100 else 0) for k in tot}=={k:sum(v[k] for v in items.values()) for k in lc}
# Add only one sheet; retain every existing sheet byte for byte.
ns='http://schemas.openxmlformats.org/spreadsheetml/2006/main';rel='http://schemas.openxmlformats.org/officeDocument/2006/relationships';pkg='http://schemas.openxmlformats.org/package/2006/relationships'
E.register_namespace('',ns);E.register_namespace('r',rel)
with zipfile.ZipFile(src) as z:files={n:z.read(n) for n in z.namelist()}
wb=E.fromstring(files['xl/workbook.xml']);rels=E.fromstring(files['xl/_rels/workbook.xml.rels']);sheets=wb.find('{'+ns+'}sheets')
assert all(x.get('name')!='Substation' for x in sheets)
sid=max(int(x.get('sheetId')) for x in sheets)+1;rid='rIdSubstationSupply';target='worksheets/substation.xml'
E.SubElement(sheets,'{'+ns+'}sheet',{'name':'Substation','sheetId':str(sid),'{'+rel+'}id':rid})
E.SubElement(rels,'{'+pkg+'}Relationship',{'Id':rid,'Type':rel+'/worksheet','Target':target})
ct=E.fromstring(files['[Content_Types].xml']);E.SubElement(ct,'{http://schemas.openxmlformats.org/package/2006/content-types}Override',{'PartName':'/xl/'+target,'ContentType':'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml'})
from xml.sax.saxutils import escape
def cell(ref,v):return '<c r="'+ref+'"'+('><v>'+str(v)+'</v></c>' if isinstance(v,(int,float)) else ' t="inlineStr"><is><t>'+escape(v)+'</t></is></c>')
rows={1:['EPC Gisagara - Substation'],3:['Transformer','Category','Rating (kVA)','Supply quantity','X (m)','Y (m)'],4:['SUBSTATION','New',100,1,p['x'],p['y']]}
x='<worksheet xmlns="'+ns+'"><dimension ref="A1:F4"/><sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="24"/><cols>'+''.join('<col min="%s" max="%s" width="%s" customWidth="1"/>'%(i,i,width) for i,width in enumerate([28,14,18,20,20,20],1))+'</cols><sheetData>'
for row,values in rows.items():x+='<row r="%s" ht="28" customHeight="1">'%row+''.join(cell(openpyxl.utils.get_column_letter(i)+str(row),v) for i,v in enumerate(values,1))+'</row>'
x+='</sheetData><mergeCells count="1"><mergeCell ref="A1:F1"/></mergeCells><pageMargins left="0.4" right="0.4" top="0.5" bottom="0.5" header="0.2" footer="0.2"/><pageSetup paperSize="9" orientation="landscape" fitToWidth="1" fitToHeight="1"/><headerFooter><oddFooter>&amp;CDS GridDesign by datasolutions.rw</oddFooter></headerFooter></worksheet>'
x=x.replace('<dimension ', '<sheetPr><pageSetUpPr fitToPage="1"/></sheetPr><dimension ',1)
files['xl/'+target]=x.encode();files['xl/workbook.xml']=E.tostring(wb,encoding='utf-8',xml_declaration=True);files['xl/_rels/workbook.xml.rels']=E.tostring(rels,encoding='utf-8',xml_declaration=True);files['[Content_Types].xml']=E.tostring(ct,encoding='utf-8',xml_declaration=True)
dst=o/src.name
with zipfile.ZipFile(dst,'w',zipfile.ZIP_DEFLATED) as z:
 for n,b in files.items():z.writestr(n,b)
check=openpyxl.load_workbook(dst,data_only=True);assert check['Substation']['D4'].value==1;assert check['Substation']['C4'].value==100
with zipfile.ZipFile(src) as a,zipfile.ZipFile(dst) as b:assert a.read('xl/worksheets/sheet1.xml')==b.read('xl/worksheets/sheet1.xml')
sh=o/'Shapefiles';sh.mkdir(exist_ok=True)
# GIS preserves the actual 100 kVA Kirwa rating and saved Fill-in nature.
with zipfile.ZipFile(o/'native/combined_transformer.shp.zip') as z:
 for m in z.infolist():
  if not m.is_dir():(sh/Path(m.filename).name).write_bytes(z.read(m))
for path in sh.glob('*.shp'):
 for ext in ['.shx','.dbf','.prj']:assert path.with_suffix(ext).exists()
assert len(list(sh.iterdir()))==32
(o/'overall_combined_report.xlsx').write_bytes((o/'retained-supply-native/combined_transformer.xlsx').read_bytes())
proof={'scope_sites':87,'main_staking_supply':64,'substation_sheet_supply':1,'staking_supply':65,'combined_supply':65,'size_totals':{k:tot[k]+(k==100) for k in tot},'matched_supply_rows_by_xy':matched,'Kirwa':'retained existing, 100 kVA plan, zero transformer supply','main_sheet_bytes_unchanged':True,'shapefile_members':32}
(o/'alignment-proof.json').write_text(json.dumps(proof,indent=2))
(o/'delivery-bridge.json').write_text(json.dumps({'feedback':'feae1d8e-271a-4a27-96cc-459b53c50ba5','workbook_projection':'Kirwa tr_size null in disposable quantity input only; planned rating100 unchanged. Native workbook has Kirwa planned100 and no transformer supply. No workbook numeric patch. Project tr_size100 unchanged.','GIS':'From unprojected native export: Kirwa tr_size100 and saved Fill-in nature preserved.','approved_plan_bridge':'6b369e94-89f2-43a0-a8c5-74b8fab0dcf7','substation':'Single100 kVA supply added on supplementary Substation sheet; main staking sheet unchanged.'},indent=2))
deliver=[dst,o/'overall_combined_report.xlsx']+list(sh.iterdir());(o/'delivery-hashes.json').write_text(json.dumps({str(f.relative_to(o)):hashlib.sha256(f.read_bytes()).hexdigest() for f in deliver},indent=2))
print(json.dumps({k:v for k,v in proof.items() if k!='matched_supply_rows_by_xy'}))




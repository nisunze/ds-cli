import pathlib,sys,json,hashlib
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');sys.path.insert(0,str(r/'pdf-libs'))
from reportlab.platypus import SimpleDocTemplate,Paragraph,Spacer,Table,TableStyle,KeepTogether
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib import colors
from reportlab.lib.pagesizes import A4
from pypdf import PdfReader
a=json.loads((r/'final-comparison.json').read_text());m=[x for x in a if x['baseline']=='Matched'];assert len(a)==84 and len(m)==83
assert not {'zihare','ruhuha','upgrade_mbeho'} & {x['transformer'] for x in a}
def name(n):
 n=n.replace('updrade_','upgrade_').replace('fill_in_','').replace('upgrade_','').replace('_',' ')
 return n.title().replace('Ep ','EP ')
counts=[x for x in m if x['poles']['old']!=x['poles']['live']];types=[x for x in m if x['pole_types']['old']!=x['pole_types']['live']]
assert len(counts)==9 and len(types)==20
styles={k:ParagraphStyle(k,fontName='Helvetica',fontSize=s,leading=l,spaceAfter=sp) for k,s,l,sp in [('title',14,17,5),('body',9,12,5),('head',10,13,5),('note',8,10,4)]}
story=[]
def para(s,k='body'):story.append(Paragraph(s,styles[k]))
para('Gisagara - Pole quantity changes','title')
para('April GeoPackages compared with the live project | 10 October 2026')
para('Initial scope only. Excluded: Zihare, Ruhuha and Upgrade Mbeho.<br/>83 sites compared. Substation has no April GeoPackage baseline.','note')
def table(rows,widths):
 t=Table(rows,colWidths=widths,hAlign='LEFT',repeatRows=1)
 t.setStyle(TableStyle([('FONTNAME',(0,0),(-1,0),'Helvetica-Bold'),('FONTNAME',(0,1),(-1,-1),'Helvetica'),('FONTSIZE',(0,0),(-1,-1),8.5),('BACKGROUND',(0,0),(-1,0),colors.HexColor('#eeeeee')),('GRID',(0,0),(-1,-1),.35,colors.HexColor('#888888')),('VALIGN',(0,0),(-1,-1),'MIDDLE'),('ALIGN',(1,0),(-1,-1),'CENTER'),('LEFTPADDING',(0,0),(-1,-1),5),('RIGHTPADDING',(0,0),(-1,-1),5),('TOPPADDING',(0,0),(-1,-1),3),('BOTTOMPADDING',(0,0),(-1,-1),3)]))
 story.append(t)
para('1. Change in total pole count - 9 transformers','head')
rows=[['Transformer','April','Live','Change']]
for x in counts:
 p=x['poles'];rows.append([name(x['transformer']),str(p['old']),str(p['live']),f"{p['live']-p['old']:+d}"])
rows.append(['TOTAL - all 83 compared sites','8,070','8,111','+41'])
table(rows,[280,75,75,97]);story.append(Spacer(1,9))
para('2. Change in pole structure types - 20 transformers','head')
para('Each cell shows April > Live. Only transformers with changed type quantities are listed.','note')
rows=[['Transformer','S140','S190','S255','Untyped']]
for x in types:
 row=[name(x['transformer'])]
 for k in ['S140','S190','S255','']:row.append(f"{x['pole_types']['old'].get(k,0)} > {x['pole_types']['live'].get(k,0)}")
 rows.append(row)
table(rows,[227,75,75,75,75]);story.append(Spacer(1,7))
para('<b>Kibungo:</b> 17 live poles have no structure type; the April file had 17 more S140 poles. This difference requires resolution.','note')
para('All 11,047 customer locations and all transformer locations are unchanged. Pole labels changed at all 83 sites. These tables cover pole counts and type quantities; LV routes differ at Rwanza Market, Kibindyi, Rebero and School Transformer.','note')
def footer(canvas,doc):
 canvas.setFont('Helvetica',7);canvas.drawString(34,22,'DS GridDesign by datasolutions.rw');canvas.drawRightString(A4[0]-34,22,f'Version 1 | {doc.page}')
out=r/'Gisagara - Pole count and structure changes.pdf'
doc=SimpleDocTemplate(str(out),pagesize=A4,rightMargin=34,leftMargin=34,topMargin=29,bottomMargin=34,title='Gisagara - Pole count and structure changes',author='DS GridDesign by datasolutions.rw')
doc.build(story,onFirstPage=footer,onLaterPages=footer)
pdf=PdfReader(out);assert len(pdf.pages)==1,len(pdf.pages)
text='\n'.join(p.extract_text() for p in pdf.pages);assert '8,070' in text and '8,111' in text and 'Kibungo' in text and 'DS GridDesign by datasolutions.rw' in text
assert '_' not in text and chr(65533) not in text
(r/'pdf-proof.json').write_text(json.dumps({'file':out.name,'bytes':out.stat().st_size,'sha256':hashlib.sha256(out.read_bytes()).hexdigest(),'pages':len(pdf.pages),'count_rows':9,'type_rows':20}))
print(json.dumps({'pages':len(pdf.pages),'bytes':out.stat().st_size,'count_rows':9,'type_rows':20}))

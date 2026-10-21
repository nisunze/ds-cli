import pathlib,openpyxl,json,re
r=pathlib.Path('/home/magese/projects/gisagara/pole-restored-combined-20261010');w=openpyxl.load_workbook(r/'native/combined_transformer.xlsx',data_only=True)
print(w.sheetnames)
for sn in w.sheetnames:
 s=w[sn]
 print(sn,s.max_row,s.max_column)
 if sn in ['LV Summary','Transformer Sizing']:
  print(list(s.values)[:4])
  print([row for row in s.values if any(str(v).upper() in ['KIRWA','UPGRADE_MBEHO','UPGRADE_KIMANA','UPDRADE_BAZENGA','RUGARAMA','TOTAL'] for v in row)])

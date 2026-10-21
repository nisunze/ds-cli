import openpyxl,pathlib,subprocess
r=pathlib.Path('/home/magese/projects/gisagara/final-aligned-20261010');p=next(r.glob('Stacking*.xlsx'));w=openpyxl.load_workbook(p);w.remove(w['Sheet1']);w.save(r/'Substation-proof.xlsx')
subprocess.run(['/home/magese/.local/bin/soffice','-env:UserInstallation=file:///tmp/gisagara-final-proof-20261010','--headless','--convert-to','pdf','--outdir',str(r),str(r/'Substation-proof.xlsx')],check=True,stdout=subprocess.PIPE)
subprocess.run(['pdftoppm','-scale-to','1500','-png','-singlefile',str(r/'Substation-proof.pdf'),str(r/'Substation-proof')],check=True)


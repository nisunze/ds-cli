import pathlib,subprocess,shutil
r=pathlib.Path('/home/magese/projects/gisagara/gpkg-comparison-20261010');p=r/'Gisagara - Pole count and structure changes.pdf';shutil.copyfile(p,r/'pole-changes.pdf')
subprocess.run(['pdftoppm','-scale-to','1600','-png','-singlefile',str(p),str(r/'pole-changes-preview')],check=True)

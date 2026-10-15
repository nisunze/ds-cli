import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {existsSync,lstatSync,mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const [platform,webInput,outputInput]=process.argv.slice(2), web=resolve(webInput||''),output=resolve(outputInput||'');
if(!['windows','linux'].includes(platform)||!webInput||!outputInput||existsSync(output))throw Error('usage: build-package.mjs windows|linux PINNED_WEB NEW_OUTPUT');
const git=(r,args)=>execFileSync('git',['-C',r,...args],{encoding:'utf8'}).trim();
const source=git(root,['rev-parse','HEAD']),webSource=git(web,['rev-parse','HEAD']);
for(const [r,sha] of [[root,source],[web,webSource]]){if(!/^[a-f0-9]{40}$/.test(sha))throw Error('invalid source');git(r,['merge-base','--is-ancestor',sha,'origin/run']);}
const files=platform==='windows'?[[root,'scripts/installation-manager/ds-install-manager.ps1','ds-install-manager.ps1'],[web,'scripts/core/install-ds-core-windows.ps1','install-ds-core-windows.ps1']]:[[root,'scripts/installation-manager/ds-install-manager.sh','ds-install-manager.sh'],[web,'static/install-core.sh','install-core.sh'],[web,'static/install.sh','install-desktop.sh']];
const members=[];
for(const [r,path,filename] of files){const full=join(r,path),bytes=readFileSync(full);if(!lstatSync(full).isFile()||lstatSync(full).isSymbolicLink()||bytes.length>128*1024)throw Error('invalid package member');const committed=execFileSync('git',['-C',r,'show',`HEAD:${path}`]);const actual=execFileSync('git',['-C',r,'hash-object',`--path=${path}`,'--stdin'],{input:bytes,encoding:'utf8'}).trim();if(actual!==git(r,['rev-parse',`HEAD:${path}`]))throw Error('manager package inputs differ from committed source');members.push({filename,size_bytes:committed.length,sha256:createHash('sha256').update(committed).digest('hex')});}
mkdirSync(output,{recursive:false});files.forEach(([r,path,name])=>writeFileSync(join(output,name),execFileSync('git',['-C',r,'show',`HEAD:${path}`]),{flag:'wx'}));
const manifest={schema:'ds.installation-manager-package/v1',platform,arch:'x86_64',source_sha:source,web_source_sha:webSource,files:members};
writeFileSync(join(output,'package.receipt.json'),JSON.stringify(manifest,null,2)+'\n',{flag:'wx'});
console.log(JSON.stringify({output,...manifest},null,2));

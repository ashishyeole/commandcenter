const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert/strict'),ts=require('typescript');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'pcc-images-'));
try{
 for(const name of ['sourceImages','themes']){const s=fs.readFileSync(path.join(__dirname,`../apps/desktop/src/${name}.ts`),'utf8');fs.writeFileSync(path.join(dir,name+'.cjs'),ts.transpileModule(s,{compilerOptions:{target:ts.ScriptTarget.ES2020,module:ts.ModuleKind.CommonJS}}).outputText);}
 const {sourceImage}=require(path.join(dir,'sourceImages.cjs')),{themeId,themes}=require(path.join(dir,'themes.cjs'));
 assert.deepEqual(sourceImage('youtube','https://www.youtube.com/watch?v=M7lc1UVf-VE&key=PRIVATE&context=secret'),{url:'https://i.ytimg.com/vi/M7lc1UVf-VE/hqdefault.jpg',kind:'video'});
 assert.deepEqual(sourceImage('youtube','https://www.youtube.com/shorts/M7lc1UVf-VE?si=tracking'),{url:'https://i.ytimg.com/vi/M7lc1UVf-VE/hqdefault.jpg',kind:'video'});
 assert.equal(sourceImage('youtube','https://youtube.com.evil.example/watch?v=M7lc1UVf-VE'),null);
 assert.equal(sourceImage('youtube','https://user:secret@youtube.com/watch?v=M7lc1UVf-VE'),null);
 assert.equal(sourceImage('youtube','file:///private/video'),null);
 assert.equal(sourceImage('youtube','https://youtube.com/watch?v=../../secret'),null);
 assert.equal(sourceImage('youtube','http://youtube.com/watch?v=M7lc1UVf-VE'),null);
 assert.deepEqual(sourceImage('github','https://github.com/vercel/geist-font?key=PRIVATE'),{url:'https://avatars.githubusercontent.com/vercel?s=128',kind:'avatar'});
 assert.equal(sourceImage('github','https://127.0.0.1/private'),null);
 assert.equal(sourceImage('models','https://github.com/vercel/geist-font'),null);
 assert.equal(themes.length,4);assert.equal(themeId('unknown'),'mono');assert.equal(themeId('midnight'),'midnight');
 console.log('13 source-image/theme assertions passed: fixed CDN destinations, dropped private query strings, rejected credentials/local or spoofed URLs, four themes and safe fallback.');
}finally{fs.rmSync(dir,{recursive:true,force:true});}

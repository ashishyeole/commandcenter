// Exercises the actual date-filter and position helpers without a browser or new dependencies.
const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),cp=require('node:child_process'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..'),temp=fs.mkdtempSync(path.join(os.tmpdir(),'pcc-ui-logic-'));
try{
 cp.execFileSync(process.execPath,[path.join(root,'node_modules/typescript/bin/tsc'),'--target','es2020','--module','commonjs','--outDir',temp,path.join(root,'apps/desktop/src/pokerDates.ts'),path.join(root,'apps/desktop/src/floatingPosition.ts')],{stdio:'pipe'});
 const {currentOrUpcoming}=require(path.join(temp,'pokerDates.js')),{clampPosition}=require(path.join(temp,'floatingPosition.js'));
 const now=Date.parse('2026-11-15T18:00:00Z'),today='2026-11-15';
 assert.equal(currentOrUpcoming({start:{date:'2026-11-12'},end:{date:'2026-11-23'}},today,now),true);
 assert.equal(currentOrUpcoming({start:{date:'2026-11-12'},end:{date:today,all_day:true}},today,now),false);
 assert.equal(currentOrUpcoming({start:{date:today},end:{date:today,at:'2026-11-15T17:00:00Z'}},today,now),false);
 assert.equal(currentOrUpcoming({start:{date:today},end:{date:today,at:'2026-11-15T19:00:00Z'}},today,now),true);
 assert.equal(currentOrUpcoming({start:{date:'2026-11-16'}},today,now),true);
 assert.equal(currentOrUpcoming({start:{date:'2026-11-14'}},today,now),false);
 assert.equal(currentOrUpcoming({start:{date:'2026-11-16'},status:'cancelled'},today,now),false);
 assert.deepEqual(clampPosition({x:-100,y:3000},900,640),{x:8,y:576});
 assert.deepEqual(clampPosition({x:NaN,y:Infinity},900,640),{x:820,y:560});
 assert.deepEqual(clampPosition({x:1000,y:900},400,300),{x:336,y:236});
 console.log('10 UI logic assertions passed: ongoing/past dates, exclusive end dates, timed events, cancelled events, invalid/stored positions and resized bounds.');
}finally{fs.rmSync(temp,{recursive:true,force:true});}

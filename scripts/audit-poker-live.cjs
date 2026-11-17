const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),ts=require('typescript');
function load(file,imports={}){const s={exports:{},require:k=>imports[k]||require(k),URL,Date,Set,Map,TextEncoder,Intl};vm.runInNewContext(ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,s);return s.exports;}
const feed=load('apps/desktop/src/discoveryFeed.ts'),logic=load('apps/desktop/src/tournamentList.ts',{'./discoveryFeed':feed});
const hub=JSON.parse(fs.readFileSync('target/poker-live/native-schedules.json','utf8'));
assert.ok(hub.streams.every(s=>s.health.status==='ok'&&s.cache.items.length));
const now=Date.now(),all=logic.tournamentList(hub,'world','',now),us=logic.tournamentList(hub,'US','',now),india=logic.tournamentList(hub,'IN','',now);
assert.ok(all.length&&us.length);assert.ok(us.every(i=>i.country_code==='US'||/\b(?:usa|united states|us)\s*$/i.test(i.location||'')));
assert.ok(india.every(i=>i.country_code==='IN'||/\bindia\s*$/i.test(i.location||'')));
assert.ok(!us.some(i=>['TW','KH','CA','IT','BS','DO'].includes(i.country_code)));
assert.ok(all.every(i=>new URL(i.url).protocol==='https:'));
const report={checked_at:new Date().toISOString(),native_fetch_at:hub.checked_at,live_provider_count:hub.streams.length,worldwide:all.length,us:us.length,india:india.length,current:all.filter(i=>logic.tournamentRunning(i,now)).length,upcoming:all.filter(i=>!logic.tournamentRunning(i,now)).length,india_coverage_note:india.length?'Live India matches present':'No India events were published by the enabled schedules; this is not exhaustive India coverage.',countries_from_publisher:[...new Set(all.map(i=>i.country_code).filter(Boolean))],filters_passed:true,accounts_used:false,api_keys_used:false,interactive_ui_tested:false};
fs.writeFileSync('target/poker-live/filter-results.json',JSON.stringify(report,null,2)+'\n');
fs.writeFileSync('target/poker-live/visible-tournaments.json',JSON.stringify(all,null,2)+'\n');
console.log(JSON.stringify(report,null,2));

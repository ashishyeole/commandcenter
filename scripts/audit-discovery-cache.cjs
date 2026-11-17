// Run the shipped feed logic against public data produced by audit-mac-dmg.py.
// This is data/logic verification, not an interactive UI test.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict'),ts=require('typescript');
const fixture=JSON.parse(fs.readFileSync('target/final-dmg-audit/public-caches.json','utf8'));
const output=ts.transpileModule(fs.readFileSync('apps/desktop/src/discoveryFeed.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const sandbox={exports:{},require,URL,Date,Set,Map,TextEncoder,Intl};vm.runInNewContext(output,sandbox);
const {aggregateFeed,filterFeed,canonicalURL,researchDocument,boundedTitle,topics,contentKinds}=sandbox.exports;
const sources=fixture.sources.filter(s=>s.stream.enabled).map(s=>({stream:s.stream,cache:{items:s.items}}));
const rows=aggregateFeed(sources),base={topic:'all',provider:'all',kind:'all',region:'world',query:'',reading:'all',days:0,sort:'balanced'};
const select=(patch={},reading={},saved=new Set())=>filterFeed(rows,{...base,...patch},reading,saved);
assert.equal(new Set(rows.map(r=>r.id)).size,rows.length,'Distinct cards must not share selection/read-state IDs');
assert.ok(rows.every(r=>r.title.trim()&&r.url&&r.id));
const byTopic={},byKind={},byProvider={};
for(const [topic] of topics.filter(([t])=>!['all','custom'].includes(t))){byTopic[topic]=select({topic}).length;assert.ok(byTopic[topic]>0,topic);}
for(const kind of contentKinds){byKind[kind]=select({kind}).length;assert.ok(byKind[kind]>0,kind);}
for(const provider of new Set(rows.map(r=>r.source))){byProvider[provider]=select({provider}).length;assert.ok(byProvider[provider]>0,provider);}
for(const region of ['US','IN']){
 const chart=select({kind:'apps',region});assert.equal(chart.length,50);
 assert.ok(chart.every(r=>r.region===region&&r.id.startsWith('apple:'+region.toLowerCase()+':')));
}
const first=select()[0];assert.ok(first);
assert.equal(select({}, {[first.id]:'dismissed'}).length,select().length-1);
assert.equal(select({reading:'read'}, {[first.id]:'read'}).length,1);
assert.equal(select({reading:'saved'}, {}, new Set([canonicalURL(first.url)])).length,1);
for(const sort of ['balanced','latest','events','title','rank'])assert.equal(select({sort}).length,select().length);
assert.equal(select({query:'no-result-'+Date.now()}).length,0);
const representative=[...new Set(rows.map(r=>r.source))].map(provider=>rows.find(r=>r.source===provider));
const document=researchDocument(representative);
for(const item of representative){assert.ok(document.includes(item.url));assert.ok(document.includes(item.verification));}
for(const item of rows)assert.ok(new TextEncoder().encode(boundedTitle(item.title)).length<=500);
const audit=JSON.parse(fs.readFileSync('target/final-dmg-audit/results.json','utf8'));
const report={checked_at:new Date().toISOString(),source_revision:audit.source_revision,dmg_sha256:audit.dmg_sha256,
 public_rows:sources.reduce((n,s)=>n+s.cache.items.length,0),aggregated_rows:rows.length,visible_current_rows:select().length,
 unique_card_ids:true,by_topic:byTopic,by_kind:byKind,by_provider:byProvider,us_chart_count:50,india_chart_count:50,
 reading_selection_and_saved_filters_passed:true,all_sort_modes_retained_rows:true,citation_document_providers:representative.length,
 bounded_saved_titles_passed:true,interactive_ui_exercised:false};
fs.writeFileSync('target/final-dmg-audit/feed-behavior.json',JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));

const crypto=require('node:crypto'),now=new Date().toISOString();
const record=(kind,title,body,url=null)=>({id:crypto.randomUUID(),kind,title:boundedTitle(title),body,pinned:false,priority:'normal',status:'active',due:null,recurrence:'none',url,created_at:now,updated_at:now,device_id:'',revision:1,deleted_at:null});
fs.writeFileSync('target/final-dmg-audit/workflow-input.json',JSON.stringify({source_revision:audit.source_revision,dmg_sha256:audit.dmg_sha256,saved:rows.map(i=>record('saved',i.title,`${i.description}\n\n${i.metric}\n${i.verification}\nFetched: ${i.fetched_at}`,i.url)),documents:representative.map(i=>record('note',`Public acceptance ${i.source}`,researchDocument([i])))}));

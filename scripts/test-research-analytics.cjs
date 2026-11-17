const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),assert=require('node:assert/strict'),ts=require('typescript');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'pcc-research-'));
try{
 const source=fs.readFileSync(path.join(__dirname,'../apps/desktop/src/researchAnalytics.ts'),'utf8');
 const file=path.join(dir,'analytics.cjs');fs.writeFileSync(file,ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2020,module:ts.ModuleKind.CommonJS}}).outputText);
 const {newestDiscoveries,measuredGrowth,topicRadar,creatorOpportunities}=require(file);
 const now=Date.parse('2026-11-15T18:00:00Z');
 const item=(id,source,title,other={})=>({id,source,title,description:'',url:`https://example.com/${id}`,metric:'',published_at:'2026-11-14T12:00:00Z',fetched_at:'2026-11-15T12:00:00Z',verification:'Synthetic fixture',...other});
 const trend=(delta,unit='stars')=>({delta,hours:24,per_day:99999,unit,observed_from:'2026-11-14T12:00:00Z',observed_to:'2026-11-15T12:00:00Z'});
 const a=item('a','github','Underwater robot',{trend:trend(2)}),b=item('b','github','Aquatic rover',{trend:trend(8)}),c=item('c','models','Robot model',{trend:trend(100,'downloads')});
 const growth=measuredGrowth([a,b,c],now);
 assert.equal(growth.find(r=>r.item.id==='a').percentile,0);
 assert.equal(growth.find(r=>r.item.id==='b').percentile,100);
 assert.equal(growth.find(r=>r.item.id==='c').percentile,null);
 assert.equal(growth.find(r=>r.item.id==='b').rate,8); // Recomputed; not the supplied per_day.
 assert.equal(measuredGrowth([item('old','github','Old',{trend:{...trend(1),observed_to:'2026-10-01T00:00:00Z'}})],now).length,0);
 assert.equal(newestDiscoveries([{...a,fetched_at:'2026-01-01T00:00:00Z',title:'Old'},a])[0].title,a.title);
 const topics=topicRadar([a,b,item('prior','arxiv','Underwater ROV',{published_at:'2026-10-25T00:00:00Z'}),item('missing','rss','Underwater research',{published_at:null}),item('future','rss','Underwater research',{published_at:'2027-01-01T00:00:00Z'}),item('substring','news','Provisioning cloud')],'robotics',now);
 const aquatic=topics.find(r=>r.topic.id==='aquatic');assert.equal(aquatic.recent,2);assert.equal(aquatic.prior,1);assert.equal(aquatic.items.length,5);assert.equal(aquatic.measured,2);
 const videos=[item('youtube:1','youtube','Irrigation sensor comparison',{metric:'Creator A',url:'https://www.youtube.com/watch?v=fixture0001'}),item('youtube:2','youtube','Irrigation sensor guide',{metric:'Creator B',url:'https://www.youtube.com/watch?v=fixture0002'})];
 assert(creatorOpportunities(videos).some(r=>r.items.length===2&&r.creators.length===2));
 assert.equal(creatorOpportunities(videos.map(r=>({...r,metric:'Same name'}))).length,0);
 assert.equal(creatorOpportunities([videos[0],videos[0]]).length,0);
 assert(creatorOpportunities(videos.map((r,i)=>({...r,id:`rss:${i}`}))).some(r=>r.items.length===2&&r.creators.length===2));
 console.log('14 research analytics assertions passed: separate metric units, peer percentiles, rate calculation, stale observations, cache deduplication, publication windows, token boundaries and cross-creator evidence.');
}finally{fs.rmSync(dir,{recursive:true,force:true});}

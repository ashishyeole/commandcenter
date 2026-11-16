import type {Discovery} from './modules';
export const researchTopics=[
 {id:'aquatic',domain:'robotics',name:'Aquatic robots & ROVs',terms:['underwater','aquatic','submersible','rov'],queries:[['arxiv','all:underwater AND cat:cs.RO'],['github','underwater robot'],['news','underwater robotics']]},
 {id:'aerial',domain:'robotics',name:'Drones & aerial autonomy',terms:['drone','aerial','uav'],queries:[['arxiv','all:drone AND cat:cs.RO'],['github','autonomous drone'],['news','drone autonomy']]},
 {id:'ground',domain:'robotics',name:'Ground rovers & navigation',terms:['rover','mobile robot','robot navigation'],queries:[['arxiv','all:"mobile robot" AND cat:cs.RO'],['github','robot navigation'],['news','autonomous rover']]},
 {id:'humanoid',domain:'robotics',name:'Humanoids & manipulation',terms:['humanoid','manipulation','robot hand','grasping'],queries:[['arxiv','all:humanoid OR all:grasping'],['github','humanoid robot'],['news','humanoid robotics']]},
 {id:'soft',domain:'robotics',name:'Soft robots & embodied intelligence',terms:['soft robot','embodied','robot learning'],queries:[['arxiv','all:"soft robot" OR all:"robot learning"'],['github','embodied robotics'],['news','soft robotics']]},
 {id:'irrigation',domain:'agriculture',name:'Irrigation & soil sensing',terms:['irrigation','soil moisture','drip irrigation'],queries:[['arxiv','all:irrigation OR all:"soil moisture"'],['github','smart irrigation'],['news','irrigation sensors']]},
 {id:'precision',domain:'agriculture',name:'Precision agriculture & crop intelligence',terms:['precision agriculture','crop','orchard','farm robot','agricultural robot'],queries:[['arxiv','all:"precision agriculture" OR all:"agricultural robot"'],['github','precision agriculture'],['news','farm robotics']]},
 {id:'solar',domain:'agriculture',name:'Solar pumps & farm energy',terms:['solar pump','agrivoltaic','farm energy'],queries:[['arxiv','all:agrivoltaic OR all:"solar pump"'],['github','solar pump irrigation'],['news','agrivoltaic']]},
 {id:'schemes',domain:'agriculture',name:'Agricultural schemes & support',terms:['agriculture scheme','farm subsidy','farmer support','pm-kusum','pm-kisan'],queries:[['news','agriculture subsidy'],['rss','https://www.nass.usda.gov/rss/news.xml']]},
] as const;
export const officialResearchSources=[
 {domain:'robotics',name:'MIT robotics research news',url:'https://news.mit.edu/topic/robotics',feed:'https://news.mit.edu/topic/mitrobotics-rss.xml',note:'Institutional research reports; claims still need independent evaluation.'},
 {domain:'robotics',name:'NASA robotics and technology sources',url:'https://www.nasa.gov/rss-feeds/',note:'Official feed directory; choose a relevant current feed at the publisher.'},
 {domain:'agriculture',name:'USDA agricultural research sources',url:'https://www.ars.usda.gov/news-events/rss-feeds/',note:'Official research-feed directory; source availability may change.'},
 {domain:'agriculture',name:'USDA NASS news & statistics',url:'https://data.nass.usda.gov/Newsroom/Syndication/News/index.php',feed:'https://www.nass.usda.gov/rss/news.xml',note:'Publisher statistics/news, not personalized trading or crop advice.'},
 {domain:'agriculture',name:'India myScheme',url:'https://www.myscheme.gov.in/',note:'Official scheme discovery portal. Open current eligibility and deadlines at the source; no eligibility inferred here.'},
 {domain:'agriculture',name:'India PM-KUSUM national portal',url:'https://pmkusum.mnre.gov.in/',note:'Official solar-pump scheme portal. Confirm current state-level rules before relying on support.'},
];
export function newestDiscoveries(rows:Discovery[]){
 const seen=new Set<string>();return [...rows].filter(r=>r&&typeof r.url==='string').sort((a,b)=>(Date.parse(b.fetched_at)||0)-(Date.parse(a.fetched_at)||0)).filter(r=>{if(seen.has(r.url))return false;seen.add(r.url);return true;}).slice(0,3000);
}
function matches(text:string,term:string){return term.includes(' ')?text.includes(term):new RegExp(`(^|[^\\p{L}\\p{N}])${term}([^\\p{L}\\p{N}]|$)`,'u').test(text);}
export function topicRadar(rows:Discovery[],domain:string,now=Date.now()){
 return researchTopics.filter(t=>t.domain===domain).map(topic=>{
  const items=newestDiscoveries(rows).filter(r=>topic.terms.some(t=>matches(`${r.title} ${r.description}`.toLowerCase(),t)));
  const age=(r:Discovery)=>now-Date.parse(r.published_at||'');
  const recent=items.filter(r=>age(r)>=0&&age(r)<14*86400000).length;
  const prior=items.filter(r=>age(r)>=14*86400000&&age(r)<28*86400000).length;
  const sources=[...new Set(items.map(r=>r.source))];
  const measured=items.filter(r=>r.trend&&Number.isFinite(r.trend.delta)&&Number.isFinite(r.trend.hours)&&r.trend.hours>0&&r.trend.delta>0&&now-Date.parse(r.trend.observed_to)>=0&&now-Date.parse(r.trend.observed_to)<14*86400000).length;
  return {topic,items,recent,prior,sources,measured,score:recent*sources.length+measured};
 }).sort((a,b)=>b.score-a.score||b.items.length-a.items.length||a.topic.name.localeCompare(b.topic.name));
}
export function measuredGrowth(rows:Discovery[],now=Date.now()){
 const measured=newestDiscoveries(rows).filter(r=>['github','models','youtube'].includes(r.source)&&r.trend&&Number.isFinite(r.trend.delta)&&Number.isFinite(r.trend.hours)&&r.trend.hours>=1/60&&Number.isFinite(r.trend.delta/r.trend.hours*24)&&now-Date.parse(r.trend.observed_to)>=0&&now-Date.parse(r.trend.observed_to)<14*86400000);
 return measured.map(item=>{
  const trend=item.trend!,unit=trend.unit||'stars',rate=trend.delta/trend.hours*24;
  const peers=measured.filter(r=>r.source===item.source&&(r.trend!.unit||'stars')===unit);
  const lower=peers.filter(r=>r.trend!.delta/r.trend!.hours*24<rate).length;
  const same=peers.filter(r=>r.trend!.delta/r.trend!.hours*24===rate).length;
  const percentile=peers.length<2?null:Math.round((lower+(same-1)/2)/(peers.length-1)*100);
  return {item,rate,unit,percentile,peers:peers.length};
 }).sort((a,b)=>(b.percentile??-1)-(a.percentile??-1)||b.item.fetched_at.localeCompare(a.item.fetched_at));
}
const stop=new Set('about after again against also before being between could doing first from have into just learn more most other over should some than that their them then there these they this those through tutorial using very video videos what when where which while with would your robot robots model models'.split(' '));
export function creatorOpportunities(rows:Discovery[]){
 const videos=newestDiscoveries(rows).filter(r=>r.source==='youtube'&&r.id.startsWith('youtube:')&&!r.id.startsWith('youtube-channel:'));
 const groups=new Map<string,Discovery[]>();
 for(const item of videos){
  const words=(item.title.toLowerCase().match(/[\p{L}\p{N}]{3,}/gu)||[]).filter(w=>!stop.has(w)).slice(0,30);
  const terms=new Set(words);for(let i=0;i<words.length-1;i++)terms.add(`${words[i]} ${words[i+1]}`);
  for(const term of terms){const bucket=groups.get(term)||[];bucket.push(item);groups.set(term,bucket);}
 }
 return [...groups].map(([term,items])=>({term,items,creators:[...new Set(items.map(r=>r.metric.trim()).filter(Boolean))]})).filter(r=>r.items.length>=2&&r.creators.length>=2).sort((a,b)=>b.creators.length-a.creators.length||b.items.length-a.items.length||b.term.length-a.term.length||a.term.localeCompare(b.term)).filter((r,i,all)=>!all.slice(0,i).some(other=>r.items.every(item=>other.items.some(v=>v.url===item.url)))).slice(0,10);
}

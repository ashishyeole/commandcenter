import type {Discovery} from './modules';
export const topics=[['all','For you'],['ai','AI & models'],['code','Code & libraries'],['robotics','Robotics'],['agriculture','Agriculture'],['poker','Poker'],['learning','Lectures & learning'],['apps','Apps & tools'],['custom','Your searches']] as const;
export const providers:Record<string,string>={crossref:'Crossref research',europepmc:'Europe PMC',patents:'Patent index (Europe PMC)',schemes:'Official India scheme announcements',github:'GitHub',models:'Hugging Face',ollama:'Ollama',arxiv:'arXiv',rss:'Publisher feeds',news:'Hacker News',youtube:'YouTube',youtube_feed:'YouTube channels',reddit:'Reddit',bluesky:'Bluesky',mastodon:'Mastodon',events:'Event schedules',calendar:'Event calendars',x:'X',linkedin:'LinkedIn',mcp:'MCP registry',apps:'App charts',import:'Imported links'};
export const contentKinds=['models','repos','news','papers','videos','social','events','apps','tools'] as const;
export function providerKind(provider:string):string {return provider==='apps'?'apps':provider==='mcp'?'tools':['models','ollama'].includes(provider)?'models':provider==='github'?'repos':provider.startsWith('youtube')?'videos':['reddit','bluesky','mastodon','x','linkedin','import'].includes(provider)?'social':['events','calendar'].includes(provider)?'events':['arxiv','crossref','europepmc','patents'].includes(provider)?'papers':'news';}
export type SourceOptions={sort:string;days:number;language:string;task:string;license:string;region:string;duration:string};
export type HubStream={id:string;name:string;topic:string;provider:string;query:string;kind:string;region:string;enabled:boolean;options:SourceOptions};
export type HubItem=Discovery&{topic?:string;kind?:string;region?:string;stream_id?:string;stream_name?:string;provider_rank?:number;start_date?:string;end_date?:string;location?:string;event_status?:string;imported?:boolean;metadata?:Record<string,unknown>;topics:string[];streams:string[]};
export type HubSource={stream:HubStream;cache:{source:string;fetched_at:string;items:HubItem[]}|null;health:{status:string;checked_at:string;count?:number;error?:string}|null};
export type HubStatus={streams:HubSource[];active:string[];auto_refresh_minutes:number;last_refresh?:{at:string;refreshed:number;failed:number;cancelled:boolean}|null};
export const emptyHub:HubStatus={streams:[],active:[],auto_refresh_minutes:0};
export function canonicalURL(raw:string){try{const u=new URL(raw);for(const k of [...u.searchParams.keys()])if(/^utm_|^(fbclid|gclid)$/i.test(k))u.searchParams.delete(k);u.hash='';if(u.hostname==='apps.apple.com'){const app=u.pathname.match(/^\/([a-z]{2})\/app\/(?:.*\/)?id(\d+)$/i);if(app)return `https://apps.apple.com/${app[1].toLowerCase()}/app/id${app[2]}`;}if(['www.youtube.com','youtube.com','youtu.be'].includes(u.hostname)){const id=u.hostname==='youtu.be'?u.pathname.slice(1):u.pathname.startsWith('/shorts/')?u.pathname.slice(8):u.searchParams.get('v');if(id&&/^[A-Za-z0-9_-]{11}$/.test(id))return `https://www.youtube.com/watch?v=${id}`;}return u.toString().replace(/\/$/,'');}catch{return raw;}}
export function aggregateFeed(sources:HubSource[]):HubItem[]{
 const rows=new Map<string,HubItem>();
 for(const {stream:s,cache} of sources)for(const item of cache?.items||[]){const key=canonicalURL(item.url)+(item.start_date?'|'+item.start_date:'');const prior=rows.get(key);const topics=[...new Set([...(prior?.topics||[]),item.topic||s.topic])],streams=[...new Set([...(prior?.streams||[]),s.name])];const region=item.region||s.region;const id=item.source==='apps'&&/^apple:\d+$/.test(item.id)&&/^[A-Z]{2}$/.test(region||'')?`apple:${region!.toLowerCase()}:${item.id.slice(6)}`:item.id;const next={...item,id,topic:item.topic||s.topic,kind:item.kind||s.kind,region:item.region||s.region,stream_id:s.id,stream_name:s.name,topics,streams};if(!prior||Date.parse(item.fetched_at)>Date.parse(prior.fetched_at))rows.set(key,next);else rows.set(key,{...prior,topics,streams});}
 return [...rows.values()];
}
export function localDate(now=new Date()){return [now.getFullYear(),String(now.getMonth()+1).padStart(2,'0'),String(now.getDate()).padStart(2,'0')].join('-');}
export function matchesRegion(i:HubItem,region:string){if(region==='world')return true;if(i.region&&i.region!=='world')return i.region===region;const text=`${i.location||''} ${i.title} ${i.description}`;return region==='IN'?/\b(india|indian|mumbai|bengaluru|bangalore|delhi|goa)\b/i.test(text):region==='US'?/\b(united states|usa|u\.s\.|las vegas|california|tulare|florida|chicago|new york|texas|north carolina|south carolina|new jersey|philadelphia)\b/i.test(text):false;}
export type FeedFilter={topic:string;provider:string;kind:string;region:string;query:string;reading:string;days:number;sort:string};
export function filterFeed(rows:HubItem[],filter:FeedFilter,reading:Record<string,string>,saved:Set<string>,now=Date.now()){
 const today=localDate(new Date(now)),q=filter.query.trim().toLowerCase();
 const matching=rows.filter(i=>{
  if(i.kind==='events'){const end=i.end_date||i.start_date||'';if(/cancel/i.test(i.event_status||'')||(end.includes('T')?(!Number.isFinite(Date.parse(end))||Date.parse(end)<now):end.slice(0,10)<today))return false;}
  if(filter.topic!=='all'&&!i.topics.includes(filter.topic))return false;
  if(filter.provider!=='all'&&i.source!==filter.provider&&!(filter.provider==='youtube_feed'&&i.source==='youtube'))return false;
  if(filter.kind!=='all'&&i.kind!==filter.kind)return false;
  if(!matchesRegion(i,filter.region))return false;
  if(filter.reading==='saved'&&!saved.has(canonicalURL(i.url)))return false;
  if(!['all','saved'].includes(filter.reading)&&(reading[i.id]||'unread')!==filter.reading)return false;
  if(filter.reading==='all'&&reading[i.id]==='dismissed')return false;
  if(q&&!`${i.title} ${i.description} ${i.metric} ${i.location||''} ${i.streams.join(' ')}`.toLowerCase().includes(q))return false;
  if(filter.days&&i.kind!=='events'){const date=Date.parse(i.published_at||'');if(!Number.isFinite(date)||date<now-filter.days*86400000||date>now+86400000)return false;}
  return true;
 });
 if(filter.sort==='balanced'){const groups=new Map<string,HubItem[]>();const topicOrder=topics.filter(t=>t[0]!=='all').map(t=>t[0] as string);for(const item of [...matching].sort((a,b)=>(a.provider_rank||1)-(b.provider_rank||1))){const key=filter.topic==='all'?(item.topic||'custom'):item.source;const list=groups.get(key)||[];list.push(item);groups.set(key,list);}const keys=filter.topic==='all'?[...topicOrder.filter(k=>groups.has(k)),...[...groups.keys()].filter(k=>!topicOrder.includes(k))]:[...groups.keys()];const out:HubItem[]=[];while(keys.some(k=>groups.get(k)!.length)){for(const key of keys){const item=groups.get(key)!.shift();if(item)out.push(item);}}return out;}
 return matching.sort((a,b)=>filter.sort==='latest'?(Date.parse(b.published_at||'')||0)-(Date.parse(a.published_at||'')||0):filter.sort==='events'?(a.start_date||'9999').localeCompare(b.start_date||'9999'):filter.sort==='title'?a.title.localeCompare(b.title):(a.provider_rank||1)-(b.provider_rank||1)||(a.stream_name||'').localeCompare(b.stream_name||''));
}
export function researchDocument(items:HubItem[]){return `# Research notes\n\n## What I learned\n\nAdd your observations, comparisons and next steps here.\n\n## Sources\n\n${items.map((i,n)=>`### ${n+1}. ${i.title}\n\n${i.description}\n\n- Source: ${i.url}\n- Provider: ${i.source}\n- Observed: ${i.metric}\n- Published: ${i.published_at||'Not reported'}\n- Fetched: ${i.fetched_at}${i.start_date?`\n- Event: ${i.start_date} to ${i.end_date||i.start_date}\n- Location: ${i.location||'See source'}`:''}\n- Provenance: ${i.verification}`).join('\n\n')}\n\n## Questions to investigate\n\n- [ ] Verify the original sources\n- [ ] Compare alternatives\n`;}

export function boundedTitle(value:string,maxBytes=500){let result='',bytes=0;for(const c of value){const size=new TextEncoder().encode(c).length;if(bytes+size>maxBytes)break;result+=c;bytes+=size;}return result;}

export const rankingLabels:Record<string,string>={stars:"Most stars",forks:"Most forks",updated:"Recently updated",downloads:"Most downloads",trendingScore:"Trending",likes:"Most liked",lastModified:"Recently updated",date:"Newest first",relevance:"Most relevant",viewCount:"Most viewed",hot:"Hot",new:"New",top:"Top"};

export function eventDatePart(raw:string){const t=Date.parse(raw);return raw.includes('T')&&Number.isFinite(t)?new Date(t).toISOString().slice(0,10):raw.slice(0,10);}
export function eventDateLabel(raw:string){if(!raw)return 'Date not reported';if(!raw.includes('T'))return raw;const t=Date.parse(raw);if(!Number.isFinite(t))return raw;return new Intl.DateTimeFormat('en-US',{timeZone:'UTC',month:'short',day:'numeric',year:'numeric',hour:'2-digit',minute:'2-digit',hour12:false}).format(new Date(t))+' UTC';}

export function metadataText(value:unknown):string {return typeof value==='object'&&value!==null?JSON.stringify(value,null,2):String(value);}

import {aggregateFeed,filterFeed,localDate,type HubItem,type HubStatus} from './discoveryFeed';

export function tournamentList(hub:HubStatus,region='world',query='',now=Date.now()):HubItem[]{
 const sources=hub.streams.filter(s=>s.stream.enabled&&s.stream.topic==='poker'&&['events','calendar'].includes(s.stream.provider));
 return filterFeed(aggregateFeed(sources),{topic:'poker',kind:'events',provider:'all',region,query,reading:'all',days:0,sort:'events'},{},new Set(),now);
}

export function tournamentRunning(item:HubItem,now=Date.now()):boolean{
 const start=item.start_date||'';
 return start.includes('T')?Number.isFinite(Date.parse(start))&&Date.parse(start)<=now:!!start&&start.slice(0,10)<=localDate(new Date(now));
}

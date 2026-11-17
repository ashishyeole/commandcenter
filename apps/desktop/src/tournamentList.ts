import {aggregateFeed,filterFeed,localDate,type HubItem,type HubStatus} from './discoveryFeed';

export function tournamentList(hub:HubStatus,region='world',query='',now=Date.now()):HubItem[]{
 const sources=hub.streams.filter(s=>s.stream.enabled&&s.stream.topic==='poker'&&['events','calendar'].includes(s.stream.provider));
 return filterFeed(aggregateFeed(sources),{topic:'poker',kind:'events',provider:'all',region:'world',query,reading:'all',days:0,sort:'events'},{},new Set(),now).filter(item=>tournamentRegion(item,region));
}

export function tournamentRunning(item:HubItem,now=Date.now()):boolean{
 const start=item.start_date||'';
 return start.includes('T')?Number.isFinite(Date.parse(start))&&Date.parse(start)<=now:!!start&&start.slice(0,10)<=localDate(new Date(now));
}

export function tournamentRegion(item:HubItem,region:string):boolean{
 if(region==='world')return true;
 if(typeof item.country_code==='string'&&item.country_code)return item.country_code.toUpperCase()===region;
 if(item.region&&item.region!=='world')return item.region===region;
 // A tournament's name/marketing text is not evidence of its venue country.
 const location=(item.location||'').trim();
 return region==='US'?/\b(?:united states(?: of america)?|usa|u\.s\.|us)\s*\)?$/i.test(location):region==='IN'?/\bindia\s*\)?$/i.test(location):false;
}

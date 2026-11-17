import {useCallback,useEffect,useState,type MouseEvent} from 'react';
import {invoke,isTauri} from '@tauri-apps/api/core';
import {ArrowUpRight,CalendarDays,X} from 'lucide-react';
import {Modal} from './Modal';
import {emptyHub,eventDateLabel,type HubStatus} from './discoveryFeed';
import {tournamentList,tournamentRunning} from './tournamentList';

export function PokerTournaments(){
 const [hub,setHub]=useState<HubStatus>(emptyHub),[expanded,setExpanded]=useState(false),[region,setRegion]=useState('world'),[query,setQuery]=useState(''),[timing,setTiming]=useState('all'),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState(''),[loaded,setLoaded]=useState(false);
 const load=useCallback(async()=>{if(isTauri())setHub(await invoke<HubStatus>('discovery_hub'));setLoaded(true);},[]);
 useEffect(()=>{void load().catch(e=>{setError(String(e));setLoaded(true);});const timer=setInterval(()=>void load().catch(e=>setError(String(e))),expanded?4000:30000);return()=>clearInterval(timer);},[load,expanded]);
 const now=Date.now(),all=tournamentList(hub,'world','',now),current=all.filter(i=>tournamentRunning(i,now)).length;
 const rows=tournamentList(hub,region,query,now).filter(i=>timing==='all'||(timing==='current'?tournamentRunning(i,now):!tournamentRunning(i,now)));
 const scheduleSources=hub.streams.filter(s=>s.stream.topic==='poker'&&['events','calendar'].includes(s.stream.provider));const sources=scheduleSources.filter(s=>s.stream.enabled);
 const failed=sources.filter(s=>s.health?.status==='error'),refreshing=busy||hub.active.length>0;
 async function refresh(){setBusy(true);setError('');setMessage('');try{const result=await invoke<{refreshed:number;failed:number}>('refresh_discovery_hub',{ids:sources.map(s=>s.stream.id)});await load();setMessage(`${result.refreshed} tournament schedules refreshed${result.failed?` · ${result.failed} unavailable`:''}.`);}catch(e){setError(String(e));}finally{setBusy(false);}}
 function schedule(event:MouseEvent<HTMLAnchorElement>,url:string){if(isTauri()){event.preventDefault();setError('');void invoke('open_discovery_reader',{url}).catch(e=>setError(String(e)));}}
 return <>
  <article className="workspace-card poker-tournaments-tile">
   <div className="item-top"><span className="pill"><CalendarDays size={14}/> Public schedules</span></div>
   <h3><button className="item-title" aria-haspopup="dialog" onClick={()=>setExpanded(true)}>Current tournaments</button></h3>
   <p>A separate tournament list with direct links to event schedules.</p>
   <p className="tournament-count">{loaded?`${current} current · ${all.length-current} upcoming`:'Loading schedules…'}</p>
   <div className="item-bottom"><small>Worldwide · no login</small><button className="text-button" onClick={()=>setExpanded(true)}>View tournament list <ArrowUpRight size={13}/></button></div>
  </article>
  {expanded&&<Modal label="Current poker tournaments" onClose={()=>setExpanded(false)}>
   <div className="modal-heading"><h2>Current & upcoming tournaments</h2><button className="icon" aria-label="Close tournament list" onClick={()=>setExpanded(false)}><X/></button></div>
   <p>Public tournament schedules, ordered by start date. Ended and cancelled events are hidden. Schedule links open the publisher directly in the app.</p>
   <div className="feed-filters">
    <label>Search tournaments<input value={query} onChange={e=>setQuery(e.target.value)} placeholder="Tournament, organizer or venue…"/></label>
    <label>Region<select value={region} onChange={e=>setRegion(e.target.value)}><option value="world">Worldwide</option><option value="US">US</option><option value="IN">India</option></select></label>
    <label>When<select value={timing} onChange={e=>setTiming(e.target.value)}><option value="all">Current & upcoming</option><option value="current">Current only</option><option value="upcoming">Upcoming only</option></select></label>
   </div>
   <div className="action-row"><span>{rows.length} tournaments · {sources.length} public schedules</span><button disabled={!isTauri()||refreshing||!sources.length} onClick={()=>void refresh()}>{refreshing?'Refreshing…':'Refresh tournaments'}</button></div>
   {failed.length>0&&<p role="status">{failed.length} schedule source{failed.length===1?' is':'s are'} unavailable. Previous results retain their fetch dates.</p>}
   <div className="tournament-table-wrap"><table className="tournament-table"><thead><tr><th scope="col">Tournament</th><th scope="col">Dates</th><th scope="col">Venue</th><th scope="col">Schedule</th></tr></thead><tbody>
    {rows.map(item=><tr key={item.id+(item.start_date||'')}><td><strong>{item.title}</strong><small>{item.stream_name}</small><span className="pill">{tournamentRunning(item,now)?'Current':'Upcoming'}</span></td><td>{eventDateLabel(item.start_date||'')}<br/>to {eventDateLabel(item.end_date||item.start_date||'')}</td><td>{item.location||'Not supplied by organizer'}</td><td><a href={item.url} target="_blank" rel="noopener noreferrer" onClick={e=>schedule(e,item.url)} aria-label={`View schedule for ${item.title}`}>View schedule <ArrowUpRight size={13}/></a><small>Fetched {new Date(item.fetched_at).toLocaleString()}</small></td></tr>)}
   </tbody></table></div>
   {!rows.length&&<p>{!loaded||refreshing?'Loading public tournaments…':isTauri()?scheduleSources.length?'No matching current or upcoming tournaments. Adjust filters or use a provider schedule below.':'No public tournament schedules configured. Add one in Discover → Sources.':'Open the desktop app to load public tournament schedules.'}</p>}
   <details><summary>Direct provider schedules</summary><ul>{scheduleSources.map(({stream})=><li key={stream.id}><a href={stream.query} target="_blank" rel="noopener noreferrer" onClick={e=>schedule(e,stream.query)}>{stream.name} <ArrowUpRight size={13}/></a></li>)}</ul></details>
   <small>Dates, venue and coverage come from the publishers. Confirm the linked schedule before making plans.</small>
   {message&&<p role="status">{message}</p>}{error&&<p className="error" role="alert">{error}</p>}
  </Modal>}
 </>;
}

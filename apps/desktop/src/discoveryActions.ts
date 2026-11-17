import {invoke} from '@tauri-apps/api/core';
import type {Item} from './domain';
import {makeRecord,type WorkspaceRecord,type WorkspaceState} from './modules';
import {boundedTitle,canonicalURL,eventDatePart,type HubItem} from './discoveryFeed';
export async function savePublicDiscovery(item:HubItem,deviceId:string):Promise<Item>{
 const {items}=await invoke<{items:Item[]}>('snapshot',{query:''});const existing=items.find(i=>i.kind==='saved'&&canonicalURL(i.url||'')===canonicalURL(item.url));if(existing)return existing;
 const now=new Date().toISOString();return invoke<Item>('save_item',{item:{id:crypto.randomUUID(),kind:'saved',title:boundedTitle(item.title),body:`${item.description}\n\n${item.metric}\n${item.verification}\nFetched: ${item.fetched_at}`,pinned:false,priority:'normal',status:'active',due:null,recurrence:'none',url:item.url,created_at:now,updated_at:now,device_id:deviceId,revision:1,deleted_at:null}});
}
export async function trackPublicEvent(item:HubItem,data:WorkspaceState,save?:(item:HubItem)=>Promise<string>):Promise<WorkspaceRecord>{
 const due=item.start_date?eventDatePart(item.start_date):'';if(!due||item.kind!=='events')throw Error('This source has no dated event to track.');
 let record=data.records.find(r=>r.fields.source===item.url&&r.fields.due===due&&['poker','agriculture','events'].includes(r.module));
 if(!record){record=makeRecord(item.topics.includes('poker')?'poker':item.topics.includes('agriculture')?'agriculture':'events',data.device_id,boundedTitle(item.title));record.summary=`${item.description}\n\nSchedule supplied by source: ${item.start_date} → ${item.end_date||item.start_date}\nLocation: ${item.location||'See source'}\n${item.verification}`;record.fields={...record.fields,source:item.url,due,end:eventDatePart(item.end_date||item.start_date||''),venue:item.location||'',location:item.location||'',next_action:'Verify current organizer dates, venue and registration before making plans'};record.tags=['discovery','event'];record.links=[save?await save(item):(await savePublicDiscovery(item,data.device_id)).id];}
 record.fields.hub_watch_id=item.id;record.fields.hub_watch_stream=item.stream_id||'';record.fields.hub_watch_snapshot=JSON.stringify({title:item.title,description:item.description,start_date:item.start_date??null,end_date:item.end_date??null,location:item.location??null,event_status:item.event_status??null,url:item.url});
 return invoke<WorkspaceRecord>('save_record',{record});
}

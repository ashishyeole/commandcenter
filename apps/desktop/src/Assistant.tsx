import type {Item} from './domain';
import type {WorkspaceState} from './modules';
import {CloudAssistant} from './CloudAssistant';
export function Assistant({data,items,reload,initialQuestion=''}:{data:WorkspaceState;items:Item[];reload:()=>Promise<void>;initialQuestion?:string;openRecord:(id:string)=>void}){
 return <><div className="notice">Model discovery is available in Discover. This command center does not install, download or run language models. Search, research organization and scheduled record briefings work without an AI account.</div><CloudAssistant initialQuestion={initialQuestion} data={data} items={items} reload={reload}/></>;
}

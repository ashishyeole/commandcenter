export type Kind = 'note' | 'task' | 'saved';
export interface Item { id:string; kind:Kind; title:string; body:string; pinned:boolean; priority:string; status:string; due:string|null; recurrence:string; url:string|null; created_at:string; updated_at:string; device_id:string; revision:number; deleted_at:string|null }
export interface Snapshot { items:Item[]; device_id:string; changes:number }
// Future providers are contracts only. No implicit network or tool execution.
export interface AIProvider { answer(question:string, approvedContext:Item[], signal:AbortSignal):Promise<{text:string; sourceIds:string[]}> }
export interface ExternalSourceConnector { fetch(signal:AbortSignal):Promise<Item[]> }
export interface SyncTransport { exchange(peerId:string, afterSequence:number, signal:AbortSignal):Promise<unknown> }
export interface BackupProvider { export(password:string):Promise<string|null> }
export interface LocalRepository { search(query:string):Promise<Snapshot>; save(item:Item):Promise<Item> }

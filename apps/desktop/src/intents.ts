// Deterministic routing only. Every mutation opens a review; remote requests keep
// their own permission gates. This parser never asks a model to execute code.
export type Intent=
 |{kind:'navigate';view:string;module?:string;query?:string;minimumGuarantee?:number;uncontacted?:boolean}
 |{kind:'capture';itemKind:'note'|'task';text:string;pinned:boolean;recurrence:string;due:string|null}
 |{kind:'edit';action:'pin'|'unpin'|'complete';title:string}
 |{kind:'discover';source:string;query:string;topic?:string;region?:string;content?:string}
 |{kind:'answer';question:string}
 |{kind:'search';query:string};
const weekdays=['sunday','monday','tuesday','wednesday','thursday','friday','saturday'];
function validDate(raw:string){const d=new Date(raw+'T12:00:00');return Number.isFinite(d.getTime())&&day(d)===raw;}
const day=(d:Date)=>[d.getFullYear(),String(d.getMonth()+1).padStart(2,'0'),String(d.getDate()).padStart(2,'0')].join('-');
export function interpretCommand(raw:string,now=new Date()):Intent{
 const input=raw.trim().slice(0,8000),lower=input.toLowerCase();
 const feed=input.match(/^(?:show|find|discover|search|research|browse|monitor|what(?:'s| is) new (?:in|with))\s+(?:me\s+)?(.+)$/i);
 if(feed && /\b(?:apps?|mcp|tools?|trending|latest|top|feeds?|news|breakthroughs?|patents?|schemes?|subsidies|lectures?|robotics|agri(?:culture|tech)?|models?|repositories|repos|videos?|tournaments?|satellites)\b/i.test(feed[1]) && !/\b(?:guarantee(?:d)?|uncontacted|not contacted)\b/i.test(feed[1])) {
  const text=feed[1],topic=/\b(apps?|app charts?|mcp|tools?)\b/i.test(text)?'apps':/\b(poker|tournaments?|satellites)\b/i.test(text)?'poker':/\b(agri(?:culture|tech)?|farming|schemes?|subsidies)\b/i.test(text)?'agriculture':/\b(robot(?:s|ics)?|humanoids?)\b/i.test(text)?'robotics':/\b(models?|llms?|hugging face|ollama)\b/i.test(text)?'ai':/\b(repos?|repositories|github|libraries)\b/i.test(text)?'code':/\b(lectures?|courses?|learning)\b/i.test(text)?'learning':'all';
  const region=/\b(india|indian)\b/i.test(text)?'IN':/\b(us|usa|united states)\b/i.test(text)?'US':'world';
  const content=/\b(mcp|tools?)\b/i.test(text)?'tools':/\b(apps?|app charts?)\b/i.test(text)?'apps':/\b(videos?|youtube|lectures?)\b/i.test(text)?'videos':/\b(papers?|research papers)\b/i.test(text)?'papers':/\b(events?|tournaments?|satellites)\b/i.test(text)?'events':/\b(models?|llms?)\b/i.test(text)?'models':/\b(repos?|repositories|libraries)\b/i.test(text)?'repos':/\b(news)\b/i.test(text)?'news':'all';
  const query=text.replace(/\b(?:the|all|my|current|upcoming|latest|top|trending|new|in|from|on|worldwide|india|indian|us|usa|united states|feeds?|news|robotics|robots?|humanoids?|agriculture|agritech|farming|poker|models?|llms?|repositories|repos?|libraries|videos?|youtube|lectures?|courses?|learning|events?|tournaments?|satellites)\b/ig,' ').replace(/\s+/g,' ').trim();
  return {kind:'discover',source:'all',query,topic,region,content};
 }
 const relative=input.match(/^(?:remind me to|create a task(?: to)?|task:)\s*(.+?)\s+(in (\d{1,3}) (days?|weeks?)|next (monday|tuesday|wednesday|thursday|friday|saturday|sunday))$/i);
 if(relative){const due=new Date(now);const offset=relative[3]?Number(relative[3])*(/^week/i.test(relative[4])?7:1):((weekdays.indexOf(relative[5].toLowerCase())-due.getDay()+7)%7||7);if(offset>0&&offset<=366){due.setDate(due.getDate()+offset);return {kind:'capture',itemKind:'task',text:relative[1],pinned:false,recurrence:'none',due:day(due)};}}

 const weekly=input.match(/^remind me every (monday|tuesday|wednesday|thursday|friday|saturday|sunday)(?: indefinitely| forever)? to (.+)$/i);
 if(weekly){const due=new Date(now);due.setDate(due.getDate()+(weekdays.indexOf(weekly[1].toLowerCase())-due.getDay()+7)%7);return {kind:'capture',itemKind:'task',text:weekly[2],pinned:false,recurrence:'weekly',due:day(due)};}
 const repeating=input.match(/^(?:remind me|task) (every day|daily|every week|weekly|every month|monthly)(?: indefinitely| forever)? to (.+)$/i);
 if(repeating)return {kind:'capture',itemKind:'task',text:repeating[2],pinned:false,recurrence:/month/i.test(repeating[1])?'monthly':/week/i.test(repeating[1])?'weekly':'daily',due:day(now)};
 const note=input.match(/^(?:take a note|create a note|note|remember)\s*:?\s+(.+)$/i)||input.match(/^note:\s*(.+)$/i);
 if(note){const pinned=/[,;]?\s+(?:and )?pin (?:it )?forever[.!]?$/i.test(note[1]);return {kind:'capture',itemKind:'note',text:note[1].replace(/[,;]?\s+(?:and )?pin (?:it )?forever[.!]?$/i,''),pinned,recurrence:'none',due:null};}
 const task=input.match(/^(?:create a task|task|remind me to)\s*:?\s+(.+)$/i)||input.match(/^task:\s*(.+)$/i);
 if(task){let text=task[1];let due:null|string=null;const when=text.match(/\s+(today|tomorrow|on \d{4}-\d{2}-\d{2})$/i);if(when){text=text.slice(0,-when[0].length);if(when[1].toLowerCase().startsWith('on '))due=validDate(when[1].slice(3))?when[1].slice(3):null;else{const d=new Date(now);if(when[1].toLowerCase()==='tomorrow')d.setDate(d.getDate()+1);due=day(d);}}return {kind:'capture',itemKind:'task',text,pinned:false,recurrence:'none',due};}
 const edit=input.match(/^(pin|unpin|complete)(?: task| note)?\s+(.+?)(?:\s+forever)?$/i);
 if(edit)return {kind:'edit',action:edit[1].toLowerCase() as 'pin'|'unpin'|'complete',title:edit[2]};
 const destinations:Record<string,string>={home:'Today',today:'Today',inbox:'Inbox',notes:'Notes',tasks:'Tasks',saved:'Saved',documents:'Documents',files:'Documents',discover:'Discover',assistant:'Assistant',settings:'Settings',vault:'Vault',timeline:'Timeline',automations:'Automations','device sync':'Device sync',devices:'Device sync',backup:'Data & privacy','data & privacy':'Data & privacy',workspaces:'Workspaces'};
 const target=lower.replace(/^(?:open|show|go to|navigate to)\s+(?:my )?/,'');
 if(destinations[target])return {kind:'navigate',view:destinations[target]};
 const modules:Record<string,string>={projects:'projects',robotics:'robotics','robotics lab':'robotics',agriculture:'agriculture','agri-tech':'agriculture',poker:'poker',crm:'crm',businesses:'crm',opportunities:'crm',companies:'companies',learning:'learning',ideas:'ideas',content:'content',travel:'events',assets:'assets',missions:'missions','mcp marketplace':'mcp',mcp:'mcp',watchlists:'watchlists'};
 if(modules[target])return {kind:'navigate',view:'Workspaces',module:modules[target]};
 if(/^(?:show|find)\b/i.test(input)&&/\b(?:businesses|leads)\b/i.test(input))return {kind:'navigate',view:'Workspaces',module:'crm',query:/\baustin\b/i.test(input)?'Austin':'',uncontacted:/not contacted|uncontacted/i.test(input)};
 if(/^(?:show|find)\b/i.test(input)&&/\b(?:poker|tournaments|satellites)\b/i.test(input)){
  const guarantee=input.match(/(?:over|above|at least)\s*\$?([\d,.]+)\s*(million|m|k)?\s*(?:guaranteed|guarantee)?/i);const amount=guarantee?Number(guarantee[1].replaceAll(',',''))*(/^(m|million)$/i.test(guarantee[2]||'')?1e6:/^k$/i.test(guarantee[2]||'')?1e3:1):undefined;
  return {kind:'navigate',view:'Workspaces',module:'poker',minimumGuarantee:amount};
 }
 if(/^(?:ask|answer|explain|summarize|why|how|what)\b/i.test(input))return {kind:'answer',question:input.replace(/^ask\s*:?\s*/i,'')};
 if(/^(?:find|discover|search|research)\b/i.test(input)){
  const query=input.replace(/^(?:find|discover|search|research)\s+/i,'');
  if(/\b(?:mcp|tools?)\b/i.test(query))return {kind:'discover',source:'mcp',query:query.replace(/\bmcp\b/ig,'').trim()};
  if(/\b(?:papers?|arxiv)\b/i.test(query))return {kind:'discover',source:'arxiv',query};
  if(/\b(?:youtube|videos?|channels?)\b/i.test(query))return {kind:'discover',source:'youtube',query};
  if(/\b(?:models?|llms?)\b/i.test(query))return {kind:'discover',source:'models',query};
  if(/\b(?:github|repos?|repositories|robots?|robotics|open.source)\b/i.test(query))return {kind:'discover',source:'github',query};
  return {kind:'search',query};
 }
 return {kind:'search',query:input};
}

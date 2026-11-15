type EventDate={date:string;at?:string;all_day?:boolean};
export function currentOrUpcoming(event:{start:EventDate;end?:EventDate;status?:string},today:string,now=Date.now()):boolean{
 if(event.status?.toUpperCase()==='CANCELLED')return false;
 const end=event.end;
 if(end?.at&&Number.isFinite(Date.parse(end.at)))return Date.parse(end.at)>now;
 if(end?.date)return end.all_day?end.date>today:end.date>=today;
 return event.start.date>=today;
}

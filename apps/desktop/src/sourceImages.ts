/** Fixed public CDN destinations only. Never forward queries, keys or workspace content. */
export function sourceImage(source:string,raw:string):{url:string;kind:'video'|'avatar'}|null{
 try{const u=new URL(raw);if(u.protocol!=='https:'||u.username||u.password)return null;
  if(source==='youtube'&&['www.youtube.com','youtube.com','youtu.be'].includes(u.hostname)){
   const id=u.hostname==='youtu.be'?u.pathname.slice(1):u.pathname==='/watch'?u.searchParams.get('v'):u.pathname.startsWith('/shorts/')?u.pathname.slice(8):null;
   if(id&&/^[A-Za-z0-9_-]{11}$/.test(id))return {url:`https://i.ytimg.com/vi/${id}/hqdefault.jpg`,kind:'video'};
  }
  if(source==='github'&&u.hostname==='github.com'){
   const [owner,repo]=u.pathname.split('/').filter(Boolean);if(owner&&repo&&/^[A-Za-z0-9-]{1,39}$/.test(owner))return {url:`https://avatars.githubusercontent.com/${owner}?s=128`,kind:'avatar'};
  }
 }catch{}return null;
}

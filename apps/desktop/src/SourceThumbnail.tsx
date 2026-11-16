import {useEffect,useState} from 'react';
import {Play} from 'lucide-react';
import {sourceImage} from './sourceImages';
export function SourceThumbnail({source,url}:{source:string;url:string}){
 const image=sourceImage(source,url),[failed,setFailed]=useState(false);useEffect(()=>setFailed(false),[url]);
 if(!image||failed)return null;
 return <div className={`source-thumbnail ${image.kind}`}><img src={image.url} alt="" loading="lazy" decoding="async" referrerPolicy="no-referrer" onError={()=>setFailed(true)}/>{image.kind==='video'&&<span className="thumbnail-play" aria-hidden="true"><Play size={18} fill="currentColor"/></span>}</div>;
}

export type Point={x:number;y:number};
export function clampPosition(p:Point,width:number,height:number):Point{return {x:Math.max(8,Math.min(Number.isFinite(p.x)?p.x:width-80,Math.max(8,width-64))),y:Math.max(8,Math.min(Number.isFinite(p.y)?p.y:height-80,Math.max(8,height-64)))};}

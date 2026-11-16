export const themes=[
 {id:'mono',name:'Monochrome',description:'Silver, paper and ink. Precise mono type.',colors:['#ededee','#ffffff','#18191b','#989a9f']},
 {id:'lumen',name:'Lumen',description:'Warm light, soft paper and a quiet teal accent.',colors:['#f6f3ec','#fffdf8','#286a60','#c6d4c7']},
 {id:'slate',name:'Slate',description:'Cool gray, clear blue and an airy workspace.',colors:['#edf1f6','#ffffff','#3a608e','#a9bad0']},
 {id:'midnight',name:'Midnight',description:'Deep charcoal, silver text and a violet accent.',colors:['#14161b','#1d2027','#b7a5e7','#657087']},
] as const;
export type ThemeId=typeof themes[number]['id'];
export function themeId(value:unknown):ThemeId{return themes.some(t=>t.id===value)?value as ThemeId:'mono';}

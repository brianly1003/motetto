import {readFileSync, writeFileSync} from 'node:fs';
const tokens=JSON.parse(readFileSync(new URL('../tokens.json',import.meta.url)));
function flatten(node,path=[]){return Object.entries(node).flatMap(([key,value])=>key.startsWith('$')?[]:value.$value!==undefined?[[[...path,key].join('-'),value]]:flatten(value,[...path,key]));}
function value(t){const v=t.$value; if(t.$type==='color')return v.hex; if(Array.isArray(v))return v.map(x=>`"${x}"`).join(','); if(v.unit)return `${v.value}${v.unit}`;return null;}
const vars=(node)=>flatten(node).map(([key,t])=>[key,value(t)]).filter(([,v])=>v!==null).map(([key,v])=>`  --${key}: ${v};`).join('\n');
const {color,...rest}=tokens;
writeFileSync(new URL('../styles/theme.css',import.meta.url),`/* Generated from ui/tokens.json. */\n:root {\n${vars(rest)}\n${vars(color.light)}\n}\n@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {\n${vars(color.dark)}\n} }\n:root[data-theme="dark"] {\n${vars(color.dark)}\n}\n`);

#!/usr/bin/env node
// Author inert extension metadata. Never install dependencies or execute tools.
import fs from 'node:fs';
const [id,name,endpoint,out]=process.argv.slice(2);
if(!id||!name||!endpoint||!out||!/^[a-zA-Z0-9._-]{1,100}$/.test(id)||name.length>100)throw Error('Usage: node scripts/create-extension.mjs ID NAME HTTPS_ENDPOINT NEW_FILE.json');
const url=new URL(endpoint);
if(url.protocol!=='https:'||url.username||url.password||url.search||url.hash)throw Error('Use a fixed public HTTPS endpoint without credentials, query or fragment');
const manifest={schema:1,id,name,version:'1.0.0',description:'Public information adapter. Review the publisher and endpoint before use.',publisher:'Local author',license:'UNLICENSED',kind:'rest',endpoint:url.href,method:'GET',image:'',command:[],tools:[]};
fs.writeFileSync(out,JSON.stringify(manifest,null,2)+'\n',{flag:'wx',mode:0o600});
console.log('Created inert manifest. Review it in Workspaces → Tools & automation → MCP → Extensions → Validate before installing. No request was sent.');

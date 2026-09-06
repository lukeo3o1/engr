const fs=require('fs');
const {canon,sha}=require('./jcs.js');
function digestOf(objectId, event){
  const {digest, ...rest}=event;
  return sha(canon({object:objectId, event:rest}));
}
module.exports={digestOf};
if(require.main===module){
  const [file,objectId,mode]=process.argv.slice(2);
  const lines=fs.readFileSync(file,'utf8').trim().split('\n');
  if(mode==='check'){
    lines.forEach((l,i)=>{const e=JSON.parse(l);
      const got=digestOf(objectId,e);
      console.log(`event ${i+1}: stored=${e.digest.slice(0,16)} computed=${got.slice(0,16)} ${got===e.digest?'MATCH':'MISMATCH'}`);});
    return;
  }
  // duplicate the last event with an uppercase id, resealed over that spelling
  const last=JSON.parse(lines[lines.length-1]);
  const alt={...last, id:last.id.toUpperCase(), rev:last.rev+1};
  delete alt.digest;
  alt.digest=digestOf(objectId, alt);
  fs.writeFileSync(file, lines.join('\n')+'\n'+canon(alt)+'\n');
  console.log('appended an event whose id is the uppercase spelling of', last.id);
  console.log('sealed over that spelling:', alt.digest.slice(0,20));
}

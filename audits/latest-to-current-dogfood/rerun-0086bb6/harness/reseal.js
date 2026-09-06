const fs = require('fs');
const { canon, sha } = require('./jcs.js');
const [file, sid, newText] = process.argv.slice(2);
const obj = JSON.parse(fs.readFileSync(file, 'utf8'));
const s = obj.sections.find(x => x.id === Number(sid));
if (!s) { console.error('no such section'); process.exit(1); }
console.log('old text:', s.text.slice(0, 60), '...');
s.text = newText;
delete s.digest;
s.digest = sha(canon(Object.fromEntries(Object.entries(s).filter(([k]) => k !== 'digest'))));
delete obj.digest;
obj.digest = sha(canon(Object.fromEntries(Object.entries(obj).filter(([k]) => k !== 'digest'))));
fs.writeFileSync(file, canon(obj), 'utf8');
console.log('new section digest:', s.digest);
console.log('new object  digest:', obj.digest);

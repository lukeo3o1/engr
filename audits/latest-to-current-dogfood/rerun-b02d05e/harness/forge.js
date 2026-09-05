// Out-of-band edits that are *correctly resealed*, so that every seal the tool
// recomputes still matches. The point of each is that integrity cannot see it
// and only the target's own admitted history can.
//
//   set <object.json> <section-id> <field> <json>   add or replace a member
//   del <object.json> <section-id>                  remove a Section, reseal
//   rawdel <object.json> <section-id>               remove a Section, do NOT reseal
const fs = require('fs');
const { canon, sha } = require('./jcs.js');

const [op, file, sid, field, value] = process.argv.slice(2);
const obj = JSON.parse(fs.readFileSync(file, 'utf8'));
const seal = o => sha(canon(Object.fromEntries(Object.entries(o).filter(([k]) => k !== 'digest'))));

if (op === 'set') {
  const s = obj.sections.find(x => x.id === Number(sid));
  s[field] = JSON.parse(value);
  delete s.digest;
  s.digest = seal(s);
  console.log(`section ${sid}: ${field} = ${value}`);
  console.log('resealed section:', s.digest);
} else if (op === 'del' || op === 'rawdel') {
  const before = obj.sections.length;
  obj.sections = obj.sections.filter(x => x.id !== Number(sid));
  console.log(`removed section ${sid}: ${before} -> ${obj.sections.length} sections`);
} else {
  console.error('unknown op'); process.exit(2);
}

if (op !== 'rawdel') {
  delete obj.digest;
  obj.digest = seal(obj);
  console.log('resealed object :', obj.digest);
} else {
  console.log('object seal left alone (the control: this is ordinary damage)');
}
fs.writeFileSync(file, canon(obj), 'utf8');

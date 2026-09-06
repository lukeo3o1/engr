const crypto = require('crypto');
function canon(v) {
  if (v === null || typeof v === 'number' || typeof v === 'boolean') return JSON.stringify(v);
  if (typeof v === 'string') return JSON.stringify(v);
  if (Array.isArray(v)) return '[' + v.map(canon).join(',') + ']';
  const keys = Object.keys(v).sort((a, b) => {
    const A = [...a], B = [...b];
    // JCS sorts by UTF-16 code units
    return a < b ? -1 : a > b ? 1 : 0;
  });
  return '{' + keys.map(k => JSON.stringify(k) + ':' + canon(v[k])).join(',') + '}';
}
function sha(s) { return '1:' + crypto.createHash('sha256').update(Buffer.from(s, 'utf8')).digest('hex'); }
module.exports = { canon, sha };
if (require.main === module) {
  const fs = require('fs');
  const obj = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
  // premise check: recompute every section digest and the object digest
  let ok = true;
  for (const s of (obj.sections || [])) {
    const { digest, ...rest } = s;
    const got = sha(canon(rest));
    const same = got === digest;
    if (!same) ok = false;
    console.log(`section ${s.id}: stored=${digest} computed=${got} ${same ? 'MATCH' : 'MISMATCH'}`);
  }
  const { digest, ...orest } = obj;
  const gotO = sha(canon(orest));
  console.log(`object   : stored=${digest} computed=${gotO} ${gotO === digest ? 'MATCH' : 'MISMATCH'}`);
  if (!ok || gotO !== digest) process.exit(1);
}

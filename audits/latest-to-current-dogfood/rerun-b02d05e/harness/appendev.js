// Append one correctly sealed Event to a stream. Used to build a history that
// parses, frames and seals perfectly and still cannot be replayed onto the
// Object it belongs to — the one route to TargetHistoryUnreplayable that is not
// simply a damaged file.
const fs = require('fs');
const { canon } = require('./jcs.js');
const { digestOf } = require('./evseal.js');

const [file, objectId, rev, type, data] = process.argv.slice(2);
const ms = Date.now();
const hex = ms.toString(16).padStart(12, '0');
const r = n => Array.from({ length: n }, () => '0123456789abcdef'[Math.floor(Math.random() * 16)]).join('');
const id = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7${r(3)}-8${r(3)}-${r(12)}`;

const event = { id, type, rev: Number(rev), data: JSON.parse(data), metadata: JSON.parse(fs.readFileSync(file, 'utf8').trim().split('\n')[0]).metadata };
event.digest = digestOf(objectId, event);
fs.appendFileSync(file, canon(event) + '\n');
console.log('appended', type, 'rev', rev, 'id', id);
console.log('sealed  ', event.digest);

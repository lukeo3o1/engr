// RefDigestContract 1, assembled by hand from #66 §6.5 and checked against the
// stored scalar. The point of the exercise is that nothing here reads engr's
// own implementation: the preimage is written out from the contract text, the
// historical values come from `git show` of the pinned commit, and a selected
// optional collection that is absent on disk is carried as JSON null.
const { canon, sha } = require('./jcs.js');
const fs = require('fs');

const stored = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const historical = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
const ref = stored.sections.find(s => s.id === Number(process.argv[4])).refs[0];
const hsec = historical.sections.find(s => s.id === Number(process.argv[5]));

const project = {
  text: () => hsec.text,
  based_on: () => (hsec.based_on ? { commit: hsec.based_on } : null),
  refs: () => (hsec.refs && hsec.refs.length ? hsec.refs : null),
  header: () => hsec.header ?? null,
  role: () => hsec.role ?? null,
  content: () => (hsec.content && hsec.content.length ? hsec.content : null),
  relations: () => (hsec.relations && hsec.relations.length ? hsec.relations : null),
  admission: () => 'human',
};
const values = {};
for (const f of ref.fields) values[f] = project[f]();

const preimage = { target: ref.target, fields: ref.fields, values, commit: ref.commit };
const computed = sha(canon(preimage));
console.log('preimage :', canon(preimage));
console.log('fields   :', JSON.stringify(ref.fields));
console.log('values   :', JSON.stringify(values).slice(0, 120) + '...');
console.log('stored   :', ref.digest);
console.log('computed :', computed);
console.log(computed === ref.digest ? 'MATCH' : 'MISMATCH');
process.exit(computed === ref.digest ? 0 : 1);

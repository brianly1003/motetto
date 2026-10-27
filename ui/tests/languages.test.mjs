import {test} from 'node:test';
import assert from 'node:assert/strict';
import {matches, nativeName, systemLanguages, withTicks, originalNotesLanguage} from '../lib/languages.ts';
test('Read in finds a language by its own name, its English name or code, accents ignored',()=>{
 assert.equal(nativeName('vi'),'Tiếng Việt');
 assert.ok(matches('ms','malay') && matches('ms','MELAYU') && matches('ms','ms') && matches('vi','tieng viet'));
 assert.ok(!matches('ms','viet'));
});
test('the computer languages map to note languages in order, once',()=>{
 assert.deepEqual(systemLanguages(['ms-MY','en-US','en-GB','fil','nb','xx']),['ms','en','tl','no']);
});
test('a translation shows the ticks notes.md has now, line by line',()=>{
 const notes='### Action items\n- [x] You: Send [00:05]\n- [ ] Them: Check';
 assert.equal(withTicks('### Tindakan\n- [ ] You: Hantar [00:05]\n- [x] Them: Semak',notes),'### Tindakan\n- [x] You: Hantar [00:05]\n- [ ] Them: Semak');
});

test('same-as-call notes use one known original language, with explicit writing taking precedence',()=>{
 assert.equal(originalNotesLanguage(null,'en'),'en');
 assert.equal(originalNotesLanguage(null,'vi'),'vi');
 assert.equal(originalNotesLanguage('ms','en'),'ms');
 assert.equal(originalNotesLanguage('en','en,vi'),'en');
 assert.equal(originalNotesLanguage(null,'en,vi'),null);
 assert.equal(originalNotesLanguage(null,null),null);
 assert.equal(originalNotesLanguage(null,' en, en '),'en');
});

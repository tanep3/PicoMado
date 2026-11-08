// MUSIC-PLAYER-20261107: execute the real renderer against a minimal DOM facade.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(`${__dirname}/../src/ui.html`, 'utf8');
const start = source.indexOf('    function renderLocalMusic(data){');
const end = source.indexOf('    function renderLocalImage(data){', start);
assert(start > 0 && end > start);
const sections = [];
const el = (tag, text, className) => ({tag, text, className, children: [], append(...children) { this.children.push(...children); }});
const sandbox = {
  el, section: (...args) => { const item = el('section', args[0]); sections.push(item); return item; },
  jobsPanel: () => {}, button: label => el('button', label),
  current: {devices: [{id: 'one'}, {id: 'two'}]}, managedCommand: () => { throw Error('render must not issue commands'); }
};
vm.createContext(sandbox);
vm.runInContext(source.slice(start, end), sandbox);
sandbox.renderLocalMusic({tracks: [{key: 'key', name: '<script>track</script>', frames: 48000}], devices: {
  one: {key: 'key', playing: false, position_ms: 144, error: {code: 'device_failed', message: '端末が音声再生の失敗を報告しました。'}},
  two: {key: 'key', playing: true, position_ms: 3000, error: null}
}});
const rows = sections[1].children;
const texts = row => [row.text, ...row.children.flatMap(texts)].filter(Boolean).join(' ');
assert.match(texts(rows[0]), /再生失敗/);
assert.match(texts(rows[0]), /0\.144秒/);
assert.match(texts(rows[0]), /端末が音声再生の失敗を報告しました。/);
assert.doesNotMatch(texts(rows[0]), /再生要求中/);
assert.match(texts(rows[1]), /再生要求中/);
assert.doesNotMatch(texts(rows[1]), /再生失敗/);
assert.equal(rows[0].children.find(node => node.tag === 'select').children[0].text, '<script>track</script> (1秒)');
console.log('local music failure UI passed');

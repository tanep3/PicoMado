// MUSIC-PLAYER-20261107: execute the real renderer and section placement without IO.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(process.env.PICOMADO_UI_SOURCE || `${__dirname}/../src/ui.html`, 'utf8');
const start = source.indexOf('    function renderLocalMusic(data){');
const end = source.indexOf('    function renderLocalImage(data){', start);
const sectionStart = source.indexOf('    function section(');
const sectionEnd = source.indexOf('    function renderHome(){', sectionStart);
assert(start > 0 && end > start && sectionStart > 0 && sectionEnd > sectionStart);
const plain = value => JSON.parse(JSON.stringify(value));
const texts = row => [row.text, ...row.children.flatMap(texts)].filter(Boolean).join(' ');
const heading = section => section.children[0].children[0].children[0].text;
const rows = section => section.children.slice(1);
const buttons = row => row.children.filter(node => node.tag === 'button');
function fixture(devices = ['one', 'two']) {
  const el = (tag, text, className) => ({tag, text, className, disabled: false, children: [], append(...children) { this.children.push(...children); }});
  const main = el('main'), commands = [];
  const sandbox = {
    el, main, jobsPanel: () => {}, button: label => el('button', label),
    current: {devices: devices.map(id => ({id}))},
    managedCommand: (plugin, command) => commands.push({plugin, command: plain(command)})
  };
  vm.createContext(sandbox);
  vm.runInContext(source.slice(sectionStart, sectionEnd) + source.slice(start, end), sandbox);
  return {main, commands, render(data) { main.children = []; sandbox.renderLocalMusic(data); return main.children; }};
}
const track = {key: 'key', name: '<script>track</script>', frames: 48000};

test('device player is before the music library even with many tracks', () => {
  const f = fixture();
  const tracks = Array.from({length: 40}, (_, n) => ({key: `key-${n}`, name: `track-${n}.wav`, frames: 48000}));
  const sections = f.render({imports: tracks.map(t => t.name), tracks});
  assert.deepEqual(sections.map(heading), ['端末ごとの再生', 'ローカル音楽']);
  assert.equal(rows(sections[0]).length, 2);
  assert.equal(sections[1].children.filter(node => node.className === 'location-row').length, 40);
  assert.deepEqual(f.commands, []);
});

test('failure, position, playback state and literal track text stay device-specific', () => {
  const f = fixture();
  const sections = f.render({tracks: [track], devices: {
    one: {key: 'key', playing: false, position_ms: 144, error: {code: 'device_failed', message: '端末が音声再生の失敗を報告しました。'}},
    two: {key: 'key', playing: true, position_ms: 3000, error: null}
  }});
  const deviceRows = rows(sections.find(section => heading(section) === '端末ごとの再生'));
  assert.match(texts(deviceRows[0]), /再生失敗/);
  assert.match(texts(deviceRows[0]), /0\.144秒/);
  assert.match(texts(deviceRows[0]), /端末が音声再生の失敗を報告しました。/);
  assert.doesNotMatch(texts(deviceRows[0]), /再生要求中/);
  assert.match(texts(deviceRows[1]), /再生要求中/);
  assert.doesNotMatch(texts(deviceRows[1]), /再生失敗/);
  assert.equal(deviceRows[0].children.find(node => node.tag === 'select').children[0].text, '<script>track</script> (1秒)');
  assert.deepEqual(f.commands, []);
});

test('play, pause and stop retain the selected track and target device', () => {
  const f = fixture();
  const sections = f.render({tracks: [track, {...track, key: 'other', name: 'other.wav'}], devices: {
    one: {key: 'key', playing: false, position_ms: 144},
    two: {key: 'other', playing: true, position_ms: 3000}
  }});
  const deviceRows = rows(sections.find(section => heading(section) === '端末ごとの再生'));
  for (const [index, row] of deviceRows.entries()) {
    const select = row.children.find(node => node.tag === 'select');
    assert.equal(select.value, index ? 'other' : 'key');
    select.value = index ? 'key' : 'other';
    for (const button of buttons(row)) { assert.equal(button.disabled, false); button.onclick(); }
  }
  assert.deepEqual(f.commands, ['one', 'two'].flatMap((device_id, index) =>
    ['play', 'pause', 'stop'].map(kind => ({plugin: 'local-music', command: {kind, device_id, key: index ? 'key' : 'other'}}))));
});

test('empty library keeps the player first and disables unavailable playback', () => {
  const f = fixture();
  const sections = f.render({});
  assert.deepEqual(sections.map(heading), ['端末ごとの再生', 'ローカル音楽']);
  for (const row of rows(sections[0])) {
    assert.match(texts(row), /未選択/);
    assert.equal(row.children.find(node => node.tag === 'select').children.length, 0);
    assert(buttons(row).every(button => button.disabled));
  }
  assert.deepEqual(f.commands, []);
});

test('unselected device can play a prepared track but cannot pause or stop', () => {
  const f = fixture(['one']);
  const sections = f.render({tracks: [track]});
  const row = rows(sections.find(section => heading(section) === '端末ごとの再生'))[0];
  assert.deepEqual(buttons(row).map(button => button.disabled), [false, true, true]);
  assert.match(texts(row), /未選択/);
  assert.deepEqual(f.commands, []);
});

test('rerender keeps one player above the library without issuing commands', () => {
  const f = fixture();
  for (const data of [{}, {tracks: [track]}, {tracks: [track], pending: true}, {}]) {
    const sections = f.render(data);
    assert.deepEqual(sections.map(heading), ['端末ごとの再生', 'ローカル音楽']);
    assert.equal(rows(sections[0]).length, 2);
  }
  assert.deepEqual(f.commands, []);
});

test('streaming library has no prepare or conversion cancel action', () => {
  const f = fixture();
  const sections = f.render({imports: ['tone.mp3'], tracks: [{key: 'tone', name: 'tone.mp3', frames: 48000, streaming: true}]});
  assert.match(texts(sections[1]), /自動で一覧に登録/);
  assert.doesNotMatch(texts(sections[1]), /再生用に変換|変換を取消す/);
  assert.equal(sections[1].children.filter(node => node.className === 'location-row').length, 1);
  const row = sections[1].children.find(node => node.className === 'location-row');
  assert.deepEqual(buttons(row).map(button => button.text), ['音源と履歴を削除']);
  assert.deepEqual(f.commands, []);
});

test('explicit pause and retained failure remain distinct from stop on rerender', () => {
  const f = fixture(['paused', 'stopped', 'failed']);
  for (let i = 0; i < 2; ++i) {
    const sections = f.render({tracks: [track], devices: {
      paused: {key: 'key', playing: false, resting: 'paused', position_ms: 250},
      stopped: {key: 'key', playing: false, resting: 'stopped', position_ms: 0},
      failed: {key: 'key', playing: false, resting: 'failed', position_ms: 100}
    }});
    const deviceRows = rows(sections[0]);
    assert.match(texts(deviceRows[0]), /一時停止中/);
    assert.match(texts(deviceRows[1]), /停止中/);
    assert.doesNotMatch(texts(deviceRows[1]), /一時停止中/);
    assert.match(texts(deviceRows[2]), /再生失敗/);
    assert.deepEqual(f.commands, []);
  }
});

test('changing the Web track selector alone never resumes an explicit pause', () => {
  const f = fixture(['one']);
  const sections = f.render({tracks: [track, {...track, key: 'other'}], devices: {
    one: {key: 'key', playing: false, resting: 'paused', position_ms: 250}
  }});
  const row = rows(sections[0])[0];
  row.children.find(node => node.tag === 'select').value = 'other';
  assert.deepEqual(f.commands, []);
  buttons(row)[0].onclick();
  assert.deepEqual(f.commands, [{plugin: 'local-music', command: {kind: 'play', device_id: 'one', key: 'other'}}]);
});

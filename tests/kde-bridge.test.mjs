import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import assert from 'node:assert/strict';
import test from 'node:test';

function harness() {
  const target = { internalId: '{test-window}', frameGeometry: {x: -800, y: 100, width: 800, height: 600},
    output: {scale: 1.5}, minimized: false, move: false, resize: false, onAllDesktops: true,
    activities: [], transientFor: null };
  let tick, nextCommand = {}, last;
  const workspace = { windowList: () => [target], activeWindow: null, currentActivity: '',
    currentDesktop: {}, windowAt: () => [target] };
  const source = readFileSync(new URL('../wayvr/src/overlays/screen/kde_bridge.js', import.meta.url), 'utf8')
    .replace('__UUID__', JSON.stringify(target.internalId)).replace('__DESTINATION__', '"test-destination"').replace('__TOKEN__', '"test-token"');
  runInNewContext(source, {workspace, QTimer: function () {
    this.timeout = {connect: callback => { tick = callback; }}; this.start = () => {}; this.stop = () => {};
  }, callDBus: (_service, _path, _interface, _method, data, callback) => {
    last = JSON.parse(data); callback(JSON.stringify(nextCommand)); nextCommand = {};
  }});
  return {target, workspace, tick: () => { tick(); return last; }, command: value => { nextCommand = value; }};
}

test('hover telemetry does not activate; only an explicit activation command does', () => {
  const h = harness();
  assert.equal(h.tick().active, false);
  assert.equal(h.workspace.activeWindow, null);
  h.command({activate: true}); h.tick();
  assert.equal(h.tick().active, true);
  assert.equal(h.workspace.activeWindow, h.target);
});

test('reports identity, logical coordinates, scaling and changes of geometry generation', () => {
  const h = harness();
  const initial = h.tick();
  assert.deepEqual(initial.geometry, [-800, 100, 800, 600]);
  assert.equal(initial.scale, 1.5);
  assert.equal(initial.uuid, '{test-window}');
  h.target.frameGeometry.x = -700;
  assert.ok(h.tick().generation > initial.generation);
});

test('closure, minimization, occlusion and popup bounds never masquerade as a safe target', () => {
  const h = harness();
  h.target.minimized = true;
  assert.equal(h.tick().visible, false);
  h.target.minimized = false;
  h.command({probe: [-600, 300], probe_seq: 1}); h.tick();
  h.workspace.windowAt = () => [{internalId: '{occluder}'}];
  assert.equal(h.tick().hit_uuid, '{occluder}');
  h.workspace.windowList = () => [h.target, {transientFor: h.target}];
  assert.equal(h.tick().transients, true);
  h.workspace.windowList = () => [];
  const closed = h.tick();
  assert.equal(closed.visible, false);
  assert.equal(closed.active, false);
});

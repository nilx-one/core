// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

'use strict';

const assert = require('node:assert/strict');
const core = require('../target/core-bindings-wasm-node/index.js');

assert.equal(core.validate_pub_dress('0x0sky'), 'valid');
assert.equal(core.validate_pub_dress('0x0Sky'), 'valid');
assert.equal(core.validate_pub_dress('0xgsky'), 'invalid_discriminator');

assert.equal(core.contract_version(), '0.1.0');
assert.equal(core.fixture_corpus_version(), '0.1.0');
assert.equal(
  core.fixture_corpus_digest(),
  'sha256_d8524ee7a22aa07164362afb4098cf37404f61ab45fcfd48aab2de2fe9016009',
);

const drive = JSON.parse(core.avaia_drive_step('', '{"type":"tap","to":"b"}', '1000', 13));
assert.equal(drive.ok, true);
assert.deepEqual(drive.commands[0], { do: 'walk', to: 'b', purpose: 'tap', grass: true });
assert.equal(
  core.avaia_drive_step('', '{"type":"tick"}', '1000', 24),
  '{"error":"invalid","ok":false}',
);
const coordinate = { longitude_e7: '305234000', latitude_e7: '504501000' };
const life = JSON.parse(core.apply_avaia_life('', '0x0sky', 'x0skai',
  JSON.stringify({ op: 'initialize', home: coordinate, position: coordinate })));
assert.equal(life.ok, true);
assert.equal(life.state.activity, 'at_home');
const tick = JSON.parse(core.apply_avaia_life(JSON.stringify(life.state), '0x0sky', 'x0skai',
  JSON.stringify({ op: 'observe', elapsed_ms: '60000', position: coordinate, motion: 'idle' })));
assert.equal(tick.state.energy, '9940');
assert.equal(tick.state.hunger, '60');
assert.deepEqual(tick.state.home, coordinate);

const outward = JSON.parse(core.avaia_proximity(4600, 0, false));
assert.equal(outward.can_reveal, true);
assert.equal(outward.level, 'restricted');
const inbound = JSON.parse(core.avaia_proximity(4600, 0, true));
assert.equal(inbound.can_reveal, false);
assert.equal(inbound.duration_ms, null);
assert.equal(JSON.parse(core.avaia_proximity(4499, 0, true)).can_reveal, true);
assert.equal(JSON.parse(core.avaia_proximity(5000, 0, false)).level, 'red');

const find = { longitude_e7: '304469000', latitude_e7: '504655000' };
const orbWorld = JSON.parse(core.orb_world(JSON.stringify({
  spills: [{
    artifact_id: 'art:seg:312346:298243:e2908:1:0',
    from: { longitude_e7: '304461000', latitude_e7: '504650000' },
    to: find,
    appeared_at: '1000',
    expires_at: '1801000',
    count: 20,
    taken: [],
  }],
  avaia: find,
}), '1000'));
assert.equal(orbWorld.ok, true);
assert.equal(orbWorld.view.orbs.filter((orb) => orb.kind === 'orb').length, 20);
assert.equal(orbWorld.view.orbs[0].lands_at, '1070');
assert.equal(orbWorld.view.next_expiry, '1801000');
assert.deepEqual(JSON.parse(core.orb_world('{}', '1000')), { error: 'invalid', ok: false });
assert.equal(core.orb_spill_version(), 3);
const cellKey = '5f1c0a7e9b2d4c6e8a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f';
const cellWorld = JSON.parse(core.orb_world(JSON.stringify({
  spills: [],
  cells: [{
    key: cellKey,
    center: { longitude_e7: '304461000', latitude_e7: '504650000' },
    landmarks: [{ longitude_e7: '304470000', latitude_e7: '504665000' }],
    appeared_at: '1000',
  }],
}), '2000'));
assert.equal(cellWorld.ok, true);
assert.equal(cellWorld.view.orbs.length, 10);
assert.equal(cellWorld.view.orbs[0].id, `orb:cell:${cellKey}:0`);
assert.equal(cellWorld.view.next_expiry, null);

const destination = JSON.parse(core.courier_destination(JSON.stringify({
  origin: { longitude_e7: '305130000', latitude_e7: '504490000' },
  candidates: [
    { point_id: 'far', coordinate: { longitude_e7: '306000000', latitude_e7: '505000000' } },
    { point_id: 'near', coordinate: { longitude_e7: '305131000', latitude_e7: '504491000' } },
  ],
})));
assert.deepEqual(destination, { ok: true, point_id: 'near' });
assert.deepEqual(JSON.parse(core.courier_destination('{}')), { error: 'invalid', ok: false });

const engine = JSON.parse(core.xping_engine_command('', '{"op":"force","forced_ms":"15000"}', '1000'));
assert.equal(engine.ok, true);
assert.equal(engine.view.energy_permille, 500);
assert.equal(engine.view.repair_seeds, '25');
assert.equal(JSON.parse(core.xping_engine_view(JSON.stringify(engine.state), '1000')).view.smoke_permille, 500);
assert.deepEqual(JSON.parse(core.xping_engine_view('', 'x')), { error: 'invalid', ok: false });
const starter = JSON.parse(core.apply_inventory_command('', '{"op":"grant_starter_seeds"}', '1000'));
const repair = JSON.parse(core.xping_repair(JSON.stringify(engine.state), JSON.stringify(starter.state), '1000'));
assert.equal(repair.outcome.seeds_spent, '25');
assert.equal(repair.inventory.seeds, '9975');
assert.equal(repair.engine.energy_permille, 1000);

const report = JSON.parse(core.core_failure_report('registration.pubdress', 'invalid_prefix'));
assert.deepEqual(report, {
  ok: true,
  report: {
    context: { code: 'invalid_prefix', contract_version: '0.1.0', surface: 'registration.pubdress' },
    errorId: 'core.registration.pubdress.invalid.prefix',
    message: 'Core rejected registration.pubdress input: invalid_prefix.',
    severity: 'error',
    tags: ['core', 'contract.0.1.0'],
  },
});
assert.deepEqual(JSON.parse(core.core_failure_report('avaia.drive', 'tick')), {
  error: 'unknown_failure',
  ok: false,
});

const pointCreate = JSON.parse(core.apply_business_point_command('', JSON.stringify({
  op: 'create',
  owner: '0xda-market',
  identity: { id: 'point-1', name: 'Zoloti Vorota' },
  location: {
    address: { country: 'UA', locality: 'Kyiv', street: 'Volodymyrska', number: '40' },
    coordinate: { longitude_e7: '305131000', latitude_e7: '504489000' },
  },
}), '1800000000'));
assert.equal(pointCreate.ok, true);
assert.equal(pointCreate.status, 'draft');
const pointPending = JSON.parse(core.apply_business_point_command(
  JSON.stringify(pointCreate.state),
  '{"op":"submit","organization_verified":true}',
  '1800000000',
));
assert.equal(pointPending.state.verification.deadline_at, '1800259200');
assert.deepEqual(
  JSON.parse(core.business_point_view(JSON.stringify(pointPending.state), '1800259200', true)),
  { ok: true, status: 'expired', visibility: 'hidden' },
);
assert.equal(
  core.apply_business_point_command('', '{"op":"withdraw"}', '01'),
  '{"error":"invalid","ok":false}',
);

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

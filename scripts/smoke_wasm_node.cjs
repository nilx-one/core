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

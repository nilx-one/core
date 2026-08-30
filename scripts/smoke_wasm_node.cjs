// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

'use strict';

const assert = require('node:assert/strict');
const core = require('../target/core-bindings-wasm-node/index.js');

assert.equal(core.contract_version(), '0.1.0');
assert.equal(core.fixture_corpus_version(), '0.1.0');
assert.equal(
  core.fixture_corpus_digest(),
  'sha256_d8524ee7a22aa07164362afb4098cf37404f61ab45fcfd48aab2de2fe9016009',
);

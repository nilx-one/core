// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0
// Asks the host where the sign-in stands, plays the matching stages, taps
// the Telegram haptic engine on each one and closes the Mini App. While
// another client is active, the chat waits for that client to agree; a
// refusal marks the request 🚫 and the Mini App stays open. It speaks the
// Mini App bridge directly: Telegram's SDK does not survive the cross-origin
// hops through AIS. Outside Telegram the bridge is absent and the page
// simply stays.
(function () {
  'use strict';
  var PROGRESS = '/api/v1/telegram/login/progress';
  var POLL = 1200, ERRORS = 3;
  var STEP = 500, CHECK = 330, HOLD = 1400;
  function send(type, data) {
    var proxy = window.TelegramWebviewProxy;
    if (proxy && proxy.postEvent) {
      proxy.postEvent(type, JSON.stringify(data || {}));
      return;
    }
    var message = JSON.stringify({ eventType: type, eventData: data || {} });
    var external = window.external;
    if (external && 'notify' in external) {
      external.notify(message);
    } else if (window.parent !== window) {
      window.parent.postMessage(message, 'https://web.telegram.org');
    }
  }
  function close() {
    send('web_app_close');
  }
  function haptic(kind) {
    send('web_app_trigger_haptic_feedback', kind === 'light'
      ? { type: 'impact', impact_style: 'light' }
      : { type: 'notification', notification_type: kind });
  }
  var back = document.getElementById('back');
  if (back) back.addEventListener('click', close);
  var result = document.getElementById('stages');
  if (!result) return;
  var approval = document.getElementById('approval');
  var note = document.getElementById('note');
  var root = document.documentElement;
  var reduced = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  send('web_app_set_header_color', { color: '#6f7fc4' });
  send('web_app_set_background_color', { color: '#6f7fc4' });

  function show(list, count) {
    list.style.setProperty('--shown', String(count));
    list.children[count - 1].classList.add('shown');
  }
  // Plays `list` from stage `from` to its end, then closes.
  function play(list, from) {
    var stages = Array.prototype.slice.call(list.children);
    if (reduced) {
      root.classList.remove('playing');
      haptic('success');
      setTimeout(close, HOLD);
      return;
    }
    root.classList.add('playing');
    stages.slice(from).forEach(function (stage, offset) {
      var index = from + offset;
      var last = index === stages.length - 1;
      setTimeout(function () { show(list, index + 1); }, offset * STEP);
      setTimeout(function () {
        stage.classList.add('done');
        haptic(last ? 'success' : 'light');
        if (last) {
          root.classList.add('finished');
          setTimeout(close, HOLD);
        }
      }, offset * STEP + CHECK);
    });
  }
  function awaitApproval(label) {
    if (!approval.hidden) return;
    result.hidden = true;
    approval.hidden = false;
    root.classList.add('playing');
    show(approval, 1);
    if (note) note.textContent = label
      ? 'Підтвердь вхід на клієнті «' + label + '».'
      : 'Підтвердь вхід на активному клієнті.';
    root.classList.add('finished');
    if (back) back.hidden = true;
  }
  function refuse(text) {
    var list = approval.hidden ? result : approval;
    result.hidden = list !== result;
    approval.hidden = list !== approval;
    root.classList.add('playing', 'finished');
    show(list, 1);
    list.children[0].classList.add('refused');
    if (note) note.textContent = text;
    if (back) back.hidden = false;
    haptic('error');
  }
  function signedIn() {
    if (approval.hidden) {
      play(result, 0);
    } else {
      approval.children[0].classList.add('done');
      haptic('light');
      if (note) note.textContent = 'Повертаю в чат.';
      root.classList.remove('finished');
      play(approval, 1);
    }
  }

  var state = null;
  try { state = new URLSearchParams(window.location.search).get('state'); } catch (e) { state = null; }
  var unavailable = 'Не вдалося перевірити вхід. Повернися в чат, щоб перевірити результат.';
  if (!state || !window.fetch) {
    refuse(unavailable);
    return;
  }
  // Until the host answers, the first stage spins; nothing is checked yet.
  root.classList.add('playing');
  show(result, 1);
  var errors = 0;
  function poll() {
    window.fetch(PROGRESS + '?state=' + encodeURIComponent(state), {
      cache: 'no-store', credentials: 'omit', referrerPolicy: 'no-referrer'
    }).then(function (response) {
      if (!response.ok) throw new Error(String(response.status));
      return response.json();
    }).then(function (progress) {
      errors = 0;
      switch (progress.stage) {
        case 'signed_in': return signedIn();
        case 'awaiting_approval':
          awaitApproval(progress.active_client);
          break;
        case 'declined':
          return refuse('Вхід не підтверджено. Чат лишився неактивним.');
        case 'failed':
          return refuse('Вхід не вдався. Надішли /login у чаті ще раз.');
        case 'waiting': break;
        default: return refuse(unavailable);
      }
      setTimeout(poll, POLL);
    }).catch(function () {
      errors += 1;
      if (errors >= ERRORS) return refuse(unavailable);
      setTimeout(poll, POLL);
    });
  }
  poll();
})();

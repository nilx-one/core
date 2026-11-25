// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0
// Plays the sign-in stages, taps the Telegram haptic engine on each one and
// closes the Mini App. It speaks the Mini App bridge directly: Telegram's SDK
// does not survive the cross-origin hops through AIS. Outside Telegram the
// bridge is absent and the page simply stays.
(function () {
  'use strict';
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
  function haptic(last) {
    send('web_app_trigger_haptic_feedback', last
      ? { type: 'notification', notification_type: 'success' }
      : { type: 'impact', impact_style: 'light' });
  }
  var back = document.getElementById('back');
  if (back) back.addEventListener('click', close);
  var list = document.getElementById('stages');
  if (!list) return;
  send('web_app_set_header_color', { color: '#6f7fc4' });
  send('web_app_set_background_color', { color: '#6f7fc4' });
  var stages = Array.prototype.slice.call(list.children);
  var STEP = 500, CHECK = 330, HOLD = 1400;
  if (window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    haptic(true);
    setTimeout(close, HOLD);
    return;
  }
  document.documentElement.classList.add('playing');
  stages.forEach(function (stage, index) {
    var last = index === stages.length - 1;
    setTimeout(function () {
      list.style.setProperty('--shown', String(index + 1));
      stage.classList.add('shown');
    }, index * STEP);
    setTimeout(function () {
      stage.classList.add('done');
      haptic(last);
      if (last) {
        document.documentElement.classList.add('finished');
        setTimeout(close, HOLD);
      }
    }, index * STEP + CHECK);
  });
})();

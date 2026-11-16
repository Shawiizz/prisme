import { isDevMode } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { appConfig } from './app/app.config';
import { App } from './app/app';
import { loadLanguage } from './app/core/i18n/i18n';
import { PHONE } from './app/core/platform';

// what a phone's screen takes is styled under this one class: `:host-context(.phone)`
document.documentElement.classList.toggle('phone', PHONE);

// the browser's own menu (back, reload, print, save the image) has nothing to do in Prisme:
// a right click does what Prisme gives it, or nothing; a field keeps cut, copy and paste
if (!isDevMode()) {
  document.addEventListener('contextmenu', (event) => {
    const target = event.target as HTMLElement | null;
    if (!target?.closest('input, textarea, [contenteditable="true"]')) event.preventDefault();
  });
}

loadLanguage()
  .then(() => bootstrapApplication(App, appConfig))
  .catch((err) => console.error(err));

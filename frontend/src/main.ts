import { bootstrapApplication } from '@angular/platform-browser';
import { appConfig } from './app/app.config';
import { App } from './app/app';
import { loadLanguage } from './app/core/i18n/i18n';
import { PHONE } from './app/core/platform';

// what a phone's screen takes is styled under this one class: `:host-context(.phone)`
document.documentElement.classList.toggle('phone', PHONE);

loadLanguage()
  .then(() => bootstrapApplication(App, appConfig))
  .catch((err) => console.error(err));

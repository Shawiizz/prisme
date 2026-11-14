import { bootstrapApplication } from '@angular/platform-browser';
import { appConfig } from './app/app.config';
import { App } from './app/app';
import { loadLanguage } from './app/core/i18n/i18n';

loadLanguage()
  .then(() => bootstrapApplication(App, appConfig))
  .catch((err) => console.error(err));

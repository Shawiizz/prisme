# Prisme's website

The landing page and the documentation, to be served at https://prisme.maelferi.com. An
[Astro](https://astro.build) site with [Starlight](https://starlight.astro.build) for the
documentation; everything is built to static files.

```
pnpm install
pnpm dev          # http://localhost:4321
pnpm build        # the site in dist/
pnpm preview      # dist/ served locally
```

- `src/pages/index.astro`, `src/pages/fr/index.astro`: the landing page, in English and
  French (`src/components/landing/`, its text in `src/i18n/landing.ts`).
- `src/content/docs/docs/`: the documentation in English; `src/content/docs/fr/docs/`: in
  French. The sidebar follows the folders (`astro.config.mjs`).
- The controller list and the actions reference are read from
  `apps/prisme-desktop/controllers/` at build time, and the download links from the latest
  GitHub release: the site never lags behind the app.
- Colours, fonts and the wordmark come from the app's design system (`design/DESIGN.md`).
